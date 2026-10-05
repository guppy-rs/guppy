// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    builder::BuilderWithHakariPackage,
    cargo_cli::CargoCli,
    errors::{ExpectedError, Result},
    helpers::regenerate_lockfile,
    output::OutputContext,
};
use camino::Utf8Path;
use guppy::graph::PackageMetadata;
use log::{error, info};
use owo_colors::OwoColorize;

pub(crate) fn publish_hakari(
    package_name: &str,
    builder: BuilderWithHakariPackage<'_>,
    pass_through: &[String],
    output: OutputContext,
) -> Result<()> {
    let hakari_package = builder.hakari_package();
    let workspace = builder.graph().workspace();
    let package = workspace
        .member_by_name(package_name)
        .map_err(|error| ExpectedError::WorkspacePackageResolveFailed { error })?;

    // Remove the workspace-hack dependency from the package if it isn't published as open source.
    let mut remove_dep = if hakari_package.publish().is_never() {
        TempRemoveDep::new(builder, package, output.clone())?
    } else {
        info!(
            "not removing dependency to {} because it is marked as published (publish != false)",
            hakari_package.name().style(output.styles.package_name)
        );
        TempRemoveDep::none()
    };

    let mut cargo_cli = CargoCli::new("publish", output.clone());
    cargo_cli.add_args(pass_through.iter().map(|arg| arg.as_str()));
    // Also set --allow-dirty because we make some changes to the working directory.
    // TODO: is there a better way to handle this?
    if !remove_dep.is_none() {
        cargo_cli.add_arg("--allow-dirty");
    }

    let workspace_dir = package
        .source()
        .workspace_path()
        .expect("package is in workspace");
    let abs_path = workspace.root().join(workspace_dir);

    let display_command = cargo_cli.display_command();

    info!(
        "{} {}\n---",
        "executing".style(output.styles.command),
        display_command
    );

    match run_cargo_publish(&cargo_cli, package.name(), &abs_path) {
        Ok(()) => remove_dep.finish(true),
        Err(error) => {
            remove_dep.finish(false)?;
            Err(error)
        }
    }
}

fn run_cargo_publish(
    cargo_cli: &CargoCli<'_>,
    package_name: &str,
    package_dir: &Utf8Path,
) -> Result<()> {
    // This is unchecked so that a non-zero exit comes back as an ExitStatus.
    let output = cargo_cli
        .to_expression()
        .dir(package_dir)
        .unchecked()
        .run()
        .map_err(|error| ExpectedError::PublishExecFailed {
            package_name: package_name.to_owned(),
            command: cargo_cli.display_command(),
            error,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ExpectedError::PublishFailed {
            package_name: package_name.to_owned(),
            command: cargo_cli.display_command(),
            exit_status: output.status,
        })
    }
}

/// RAII guard to ensure packages are re-added after being published.
#[derive(Debug)]
struct TempRemoveDep<'g> {
    inner: Option<TempRemoveDepInner<'g>>,
}

impl<'g> TempRemoveDep<'g> {
    fn new(
        builder: BuilderWithHakariPackage<'g>,
        package: PackageMetadata<'g>,
        output: OutputContext,
    ) -> Result<Self> {
        let hakari_package = builder.hakari_package();
        let package_set = package.to_package_set();
        let remove_ops = builder.remove_dep_ops(&package_set, false);
        let inner = if remove_ops.is_empty() {
            info!(
                "dependency from {} to {} not present",
                package.name().style(output.styles.package_name),
                hakari_package.name().style(output.styles.package_name),
            );
            None
        } else {
            info!(
                "removing dependency from {} to {}",
                package.name().style(output.styles.package_name),
                hakari_package.name().style(output.styles.package_name),
            );
            remove_ops
                .apply()
                .map_err(|error| ExpectedError::PublishDepRemoveFailed {
                    package_name: package.name().to_owned(),
                    hakari_package_name: hakari_package.name().to_owned(),
                    error,
                })?;
            Some(TempRemoveDepInner {
                builder,
                package,
                output,
            })
        };

        Ok(Self { inner })
    }

    fn none() -> Self {
        Self { inner: None }
    }

    fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    fn finish(&mut self, success: bool) -> Result<()> {
        match self.inner.take() {
            Some(inner) => inner.finish(success),
            None => {
                // No operations need to be performed or `finish` was already called.
                Ok(())
            }
        }
    }
}

impl Drop for TempRemoveDep<'_> {
    fn drop(&mut self) {
        // Ignore errors in this impl.
        let _ = self.finish(false);
    }
}

#[derive(Debug)]
struct TempRemoveDepInner<'g> {
    builder: BuilderWithHakariPackage<'g>,
    package: PackageMetadata<'g>,
    output: OutputContext,
}

impl TempRemoveDepInner<'_> {
    fn finish(self, success: bool) -> Result<()> {
        let package_set = self.package.to_package_set();
        let add_ops = self.builder.add_dep_ops(&package_set, true);

        if success {
            info!(
                "re-adding dependency from {} to {}",
                self.package.name().style(self.output.styles.package_name),
                self.builder
                    .hakari_package()
                    .name()
                    .style(self.output.styles.package_name),
            );
        } else {
            eprintln!("---");
            error!("execution failed, rolling back changes");
        }

        add_ops
            .apply()
            .map_err(|error| ExpectedError::PublishDepRestoreFailed {
                package_name: self.package.name().to_owned(),
                hakari_package_name: self.builder.hakari_package().name().to_owned(),
                error,
            })?;
        regenerate_lockfile(self.output)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_helpers::write_executable_script;
    use crate::test_helpers::{output_context, reverse_dep_builder};
    use hakari::cli_ops::{ApplyError, ApplyErrorKind};
    use std::io;
    use tempfile::TempDir;

    // This depends on hrd-workspace-hack in the fixture.
    const MEMBER_WITH_DEP: &str = "hrd-member-normal";
    // This doesn't depend on hrd-workspace-hack in the fixture.
    const MEMBER_WITHOUT_DEP: &str = "hrd-member-unlinked";
    const HAKARI_PACKAGE: &str = "hrd-workspace-hack";

    fn workspace_member<'g>(
        builder: &BuilderWithHakariPackage<'g>,
        name: &str,
    ) -> PackageMetadata<'g> {
        builder
            .graph()
            .workspace()
            .member_by_name(name)
            .expect("package is a workspace member in the fixture")
    }

    // The fixture's workspace root is not on disk, so apply stops at its first
    // step: canonicalizing the root.
    #[track_caller]
    fn assert_missing_workspace_root(apply_error: &ApplyError, workspace_root: &Utf8Path) {
        assert_eq!(apply_error.path(), workspace_root);
        let ApplyErrorKind::CanonicalizeWorkspaceRoot { error: io_error } = apply_error.kind()
        else {
            panic!("expected CanonicalizeWorkspaceRoot, found {apply_error:?}");
        };
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    // This fails before any manifest is edited and before cargo is run.
    //
    // Don't call publish_hakari with a known package here! hrd-workspace-hack
    // is publishable, so that would get as far as spawning the real cargo, in a
    // package directory that is not on disk.
    #[test]
    fn unknown_package_is_rejected() {
        let error = publish_hakari("nope", reverse_dep_builder(), &[], output_context())
            .expect_err("nope is not a workspace member");
        let ExpectedError::WorkspacePackageResolveFailed { error: guppy_error } = &error else {
            panic!("expected WorkspacePackageResolveFailed, found {error:?}");
        };
        let guppy::Error::UnknownWorkspaceName(name) = guppy_error else {
            panic!("expected UnknownWorkspaceName, found {guppy_error:?}");
        };
        assert_eq!(name, "nope");
    }

    #[test]
    fn remove_dep_failure() {
        let builder = reverse_dep_builder();
        let workspace_root = builder.graph().workspace().root();
        let package = workspace_member(&builder, MEMBER_WITH_DEP);

        let error = TempRemoveDep::new(builder, package, output_context())
            .expect_err("the fixture's workspace root doesn't exist");
        let ExpectedError::PublishDepRemoveFailed {
            package_name,
            hakari_package_name,
            error: apply_error,
        } = &error
        else {
            panic!("expected PublishDepRemoveFailed, found {error:?}");
        };
        assert_eq!(package_name, MEMBER_WITH_DEP);
        assert_eq!(hakari_package_name, HAKARI_PACKAGE);
        assert_missing_workspace_root(apply_error, workspace_root);
    }

    #[test]
    fn remove_dep_not_present() {
        let builder = reverse_dep_builder();
        let package = workspace_member(&builder, MEMBER_WITHOUT_DEP);

        // This succeeds even though the workspace root doesn't exist: with no
        // dependency there is nothing to apply.
        let mut remove_dep = TempRemoveDep::new(builder, package, output_context())
            .expect("there is no dependency to remove");
        assert!(remove_dep.is_none());
        remove_dep
            .finish(true)
            .expect("there is no dependency to re-add");
    }

    #[test]
    fn restore_dep_failure() {
        // In both success and failure cases, ensure that the dependency is
        // re-added.
        for success in [true, false] {
            let builder = reverse_dep_builder();
            let workspace_root = builder.graph().workspace().root();
            let package = workspace_member(&builder, MEMBER_WITH_DEP);
            let mut remove_dep = TempRemoveDep {
                inner: Some(TempRemoveDepInner {
                    builder,
                    package,
                    output: output_context(),
                }),
            };

            let error = remove_dep
                .finish(success)
                .expect_err("the fixture's workspace root doesn't exist");
            let ExpectedError::PublishDepRestoreFailed {
                package_name,
                hakari_package_name,
                error: apply_error,
            } = &error
            else {
                panic!("expected PublishDepRestoreFailed for {success}, found {error:?}");
            };
            assert_eq!(package_name, MEMBER_WITH_DEP);
            assert_eq!(hakari_package_name, HAKARI_PACKAGE);
            assert_missing_workspace_root(apply_error, workspace_root);

            assert!(remove_dep.is_none());
            remove_dep
                .finish(success)
                .expect("finish does nothing the second time");
        }
    }

    #[test]
    fn cargo_publish_spawn_failure() {
        let dir = TempDir::new().expect("created temp dir");
        let package_dir: &Utf8Path = dir.path().try_into().expect("path is UTF-8");
        let cargo_program = dir.path().join("missing-cargo").into_os_string();
        let mut cargo_cli =
            CargoCli::with_cargo_program("publish", output_context(), cargo_program.clone());
        cargo_cli.add_arg("--dry-run");

        let error = run_cargo_publish(&cargo_cli, MEMBER_WITH_DEP, package_dir)
            .expect_err("cargo program doesn't exist");
        let ExpectedError::PublishExecFailed {
            package_name,
            command,
            error: io_error,
        } = &error
        else {
            panic!("expected PublishExecFailed, found {error:?}");
        };
        assert_eq!(package_name, MEMBER_WITH_DEP);
        assert_eq!(
            command,
            &format!("{} publish --dry-run", cargo_program.display()),
        );
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    // (Unix-only because the fake cargo is a shell script.)
    #[cfg(unix)]
    #[test]
    fn cargo_publish_exit_status() {
        let dir = TempDir::new().expect("created temp dir");
        let root: &Utf8Path = dir.path().try_into().expect("path is UTF-8");
        let package_dir = root.join("package");
        std::fs::create_dir(&package_dir).expect("created package dir");

        // The script records its arguments in the directory it is run in --
        // this ensures that cargo runs in the package directory.
        let succeeding = dir.path().join("cargo-succeeding");
        write_executable_script(&succeeding, "#!/bin/sh\necho \"$*\" > args\nexit 0\n");
        let mut cargo_cli =
            CargoCli::with_cargo_program("publish", output_context(), succeeding.into_os_string());
        cargo_cli.add_arg("--dry-run");
        run_cargo_publish(&cargo_cli, MEMBER_WITH_DEP, &package_dir)
            .expect("fake cargo exits with 0");
        assert_eq!(
            std::fs::read_to_string(package_dir.join("args"))
                .expect("fake cargo ran in the package directory"),
            "--color=never publish --dry-run\n",
        );

        let failing = dir.path().join("cargo-failing");
        write_executable_script(&failing, "#!/bin/sh\nexit 101\n");
        let mut cargo_cli = CargoCli::with_cargo_program(
            "publish",
            output_context(),
            failing.clone().into_os_string(),
        );
        cargo_cli.add_arg("--dry-run");
        let error = run_cargo_publish(&cargo_cli, MEMBER_WITH_DEP, &package_dir)
            .expect_err("fake cargo exits with 101");
        let ExpectedError::PublishFailed {
            package_name,
            command,
            exit_status,
        } = &error
        else {
            panic!("expected PublishFailed, found {error:?}");
        };
        assert_eq!(package_name, MEMBER_WITH_DEP);
        assert_eq!(command, &format!("{} publish --dry-run", failing.display()));
        assert_eq!(exit_status.code(), Some(101));
    }
}
