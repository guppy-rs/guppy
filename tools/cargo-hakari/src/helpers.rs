// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    cargo_cli::CargoCli,
    errors::{ExpectedError, Result},
    output::OutputContext,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::io;

/// Read the contents of the first file that matches and is present. Errors out.
pub(crate) fn read_config_contents(
    root: &Utf8Path,
    rel_paths: impl IntoIterator<Item = impl AsRef<Utf8Path>>,
) -> Result<(Utf8PathBuf, String)> {
    let mut paths_tried = Vec::new();
    for path in rel_paths {
        let abs_path = root.join(path);
        match std::fs::read_to_string(&abs_path) {
            Ok(contents) => return Ok((abs_path, contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // The path wasn't found -- continue to the next one.
                paths_tried.push(abs_path);
            }
            Err(error) => {
                return Err(ExpectedError::ConfigReadFailed {
                    config_path: abs_path,
                    error,
                });
            }
        }
    }

    Err(ExpectedError::ConfigNotFound { paths_tried })
}

/// Regenerate the lockfile after dependency updates.
pub(crate) fn regenerate_lockfile(output: OutputContext) -> Result<()> {
    // This seems to be the cheapest way to update the lockfile.
    // cargo update -p <hakari-package> can sometimes cause unnecessary index updates.
    run_lockfile_update(&CargoCli::new("tree", output))
}

fn run_lockfile_update(cargo_cli: &CargoCli<'_>) -> Result<()> {
    // This is unchecked so that a non-zero exit comes back as an ExitStatus.
    let output = cargo_cli
        .to_expression()
        .stdout_null()
        .unchecked()
        .run()
        .map_err(|error| ExpectedError::LockfileUpdateExecFailed {
            command: cargo_cli.display_command(),
            error,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ExpectedError::LockfileUpdateFailed {
            command: cargo_cli.display_command(),
            exit_status: output.status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::output_context;
    #[cfg(unix)]
    use crate::test_helpers::write_executable_script;
    use std::convert::TryInto;
    use tempfile::TempDir;

    #[test]
    fn test_read_config_contents() {
        let dir = TempDir::new().expect("created temp dir");
        let root: &Utf8Path = dir.path().try_into().expect("path is UTF-8");
        std::fs::write(dir.path().join("foo"), "foo-contents").expect("wrote foo");
        std::fs::write(dir.path().join("bar"), "bar-contents").expect("wrote bar");
        std::fs::create_dir(dir.path().join("unreadable")).expect("created unreadable");

        assert_eq!(
            read_config_contents(root, ["foo", "bar"]).expect("foo is present"),
            (root.join("foo"), "foo-contents".to_owned()),
        );
        assert_eq!(
            read_config_contents(root, ["bar", "foo"]).expect("bar is present"),
            (root.join("bar"), "bar-contents".to_owned()),
        );
        assert_eq!(
            read_config_contents(root, ["missing", "foo"]).expect("foo is present"),
            (root.join("foo"), "foo-contents".to_owned()),
        );

        let error = read_config_contents(root, ["missing", "missing-2"])
            .expect_err("neither path is present");
        let ExpectedError::ConfigNotFound { paths_tried } = &error else {
            panic!("expected ConfigNotFound, found {error:?}");
        };
        assert_eq!(
            paths_tried,
            &[root.join("missing"), root.join("missing-2")],
            "paths are absolute and in the order tried",
        );

        let error = read_config_contents(root, Vec::<&Utf8Path>::new())
            .expect_err("there are no paths to try");
        let ExpectedError::ConfigNotFound { paths_tried } = &error else {
            panic!("expected ConfigNotFound, found {error:?}");
        };
        assert_eq!(paths_tried, &Vec::<Utf8PathBuf>::new());

        // Reading a directory fails with a non-NotFound error, which stops the
        // search.
        let error = read_config_contents(root, ["missing", "unreadable", "foo"])
            .expect_err("a directory can't be read as a file");
        let ExpectedError::ConfigReadFailed {
            config_path,
            error: io_error,
        } = &error
        else {
            panic!("expected ConfigReadFailed, found {error:?}");
        };
        assert_eq!(config_path, &root.join("unreadable"));
        assert_ne!(io_error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn lockfile_update_spawn_failure() {
        let dir = TempDir::new().expect("created temp dir");
        let cargo_program = dir.path().join("missing-cargo").into_os_string();
        let cargo_cli =
            CargoCli::with_cargo_program("tree", output_context(), cargo_program.clone());

        let error = run_lockfile_update(&cargo_cli).expect_err("cargo program doesn't exist");
        let ExpectedError::LockfileUpdateExecFailed {
            command,
            error: io_error,
        } = &error
        else {
            panic!("expected LockfileUpdateExecFailed, found {error:?}");
        };
        assert_eq!(command, &format!("{} tree", cargo_program.display()));
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    // (Unix-only because the fake cargo is a shell script.)
    #[cfg(unix)]
    #[test]
    fn lockfile_update_exit_status() {
        let dir = TempDir::new().expect("created temp dir");

        let succeeding = dir.path().join("cargo-succeeding");
        write_executable_script(&succeeding, "#!/bin/sh\nexit 0\n");
        let cargo_cli =
            CargoCli::with_cargo_program("tree", output_context(), succeeding.into_os_string());
        run_lockfile_update(&cargo_cli).expect("fake cargo exits with 0");

        let failing = dir.path().join("cargo-failing");
        write_executable_script(&failing, "#!/bin/sh\nexit 101\n");
        let cargo_cli = CargoCli::with_cargo_program(
            "tree",
            output_context(),
            failing.clone().into_os_string(),
        );
        let error = run_lockfile_update(&cargo_cli).expect_err("fake cargo exits with 101");
        let ExpectedError::LockfileUpdateFailed {
            command,
            exit_status,
        } = &error
        else {
            panic!("expected LockfileUpdateFailed, found {error:?}");
        };
        assert_eq!(command, &format!("{} tree", failing.display()));
        assert_eq!(exit_status.code(), Some(101));
    }
}
