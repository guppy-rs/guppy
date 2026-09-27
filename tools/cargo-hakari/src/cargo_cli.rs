// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cargo CLI support.

use crate::output::OutputContext;
use std::{env, ffi::OsString};

pub(crate) fn cargo_program() -> OsString {
    cargo_program_from_env_var(env::var_os("CARGO"))
}

fn cargo_program_from_env_var(cargo_env_var: Option<OsString>) -> OsString {
    cargo_env_var.unwrap_or_else(|| OsString::from("cargo"))
}

#[derive(Clone, Debug)]
pub(crate) struct CargoCli<'a> {
    // This is deliberately not a PathBuf: duct prepends ./ to those, skipping
    // PATH.
    cargo_program: OsString,
    output: OutputContext,
    command: &'a str,
    args: Vec<&'a str>,
}

impl<'a> CargoCli<'a> {
    pub(crate) fn new(command: &'a str, output: OutputContext) -> Self {
        Self::with_cargo_program(command, output, cargo_program())
    }

    fn with_cargo_program(
        command: &'a str,
        output: OutputContext,
        cargo_program: OsString,
    ) -> Self {
        Self {
            cargo_program,
            output,
            command,
            args: vec![],
        }
    }

    pub(crate) fn add_arg(&mut self, arg: &'a str) -> &mut Self {
        self.args.push(arg);
        self
    }

    pub(crate) fn add_args(&mut self, args: impl IntoIterator<Item = &'a str>) -> &mut Self {
        self.args.extend(args);
        self
    }

    pub(crate) fn display_command(&self) -> String {
        let mut display_command = format!("{} {}", self.cargo_program.display(), self.command);
        for arg in &self.args {
            display_command.push(' ');
            display_command.push_str(arg);
        }
        display_command
    }

    pub(crate) fn to_expression(&self) -> duct::Expression {
        let mut initial_args = vec![];
        if self.output.quiet {
            initial_args.push("--quiet");
        }
        if self.output.verbose {
            initial_args.push("--verbose");
        }
        initial_args.push(self.output.color.to_arg());

        initial_args.push(self.command);

        duct::cmd(
            &self.cargo_program,
            initial_args.into_iter().chain(self.args.iter().copied()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{Color, Styles};
    use std::sync::Arc;
    #[cfg(unix)]
    use std::{
        fs,
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
        path::Path,
    };

    fn output_context() -> OutputContext {
        OutputContext {
            quiet: false,
            verbose: false,
            color: Color::Never,
            styles: Arc::new(Styles::default()),
        }
    }

    #[test]
    fn display_command_shows_program() {
        let mut fallback = CargoCli::with_cargo_program(
            "publish",
            output_context(),
            cargo_program_from_env_var(None),
        );
        fallback.add_args(["--dry-run", "--no-verify"]);
        assert_eq!(
            fallback.display_command(),
            "cargo publish --dry-run --no-verify",
        );

        let from_env_var = CargoCli::with_cargo_program(
            "tree",
            output_context(),
            cargo_program_from_env_var(Some(OsString::from("/opt/rust/bin/cargo"))),
        );
        assert_eq!(from_env_var.display_command(), "/opt/rust/bin/cargo tree");
    }

    #[cfg(unix)]
    #[test]
    fn display_command_non_utf8_program() {
        let non_utf8 = CargoCli::with_cargo_program(
            "tree",
            output_context(),
            cargo_program_from_env_var(Some(OsString::from_vec(
                b"/opt/rust-\xff/bin/cargo".to_vec(),
            ))),
        );
        assert_eq!(
            non_utf8.display_command(),
            "/opt/rust-\u{FFFD}/bin/cargo tree"
        );
    }

    #[cfg(unix)]
    fn write_fake_cargo(path: &Path, label: &str) {
        fs::write(path, format!("#!/bin/sh\necho \"{label}: $*\"\n"))
            .expect("wrote fake cargo script");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .expect("made fake cargo script executable");
    }

    // Unix only because Windows PATH lookup needs a real .exe.
    #[cfg(unix)]
    #[test]
    fn to_expression_resolves_program() {
        let temp_dir = tempfile::tempdir().expect("created temp dir");
        let path_dir = temp_dir.path().join("path-bin");
        let env_var_dir = temp_dir.path().join("env-var-bin");
        fs::create_dir(&path_dir).expect("created PATH dir");
        fs::create_dir(&env_var_dir).expect("created CARGO dir");
        write_fake_cargo(&path_dir.join("cargo"), "path");
        write_fake_cargo(&path_dir.join("cargo-custom"), "custom");
        write_fake_cargo(&env_var_dir.join("cargo"), "env-var");

        let run_fake_cargo = |cargo_env_var: Option<OsString>| {
            let cargo_cli = CargoCli::with_cargo_program(
                "tree",
                output_context(),
                cargo_program_from_env_var(cargo_env_var.clone()),
            );
            cargo_cli
                .to_expression()
                .env("PATH", &path_dir)
                .read()
                .unwrap_or_else(|error| {
                    panic!("fake cargo ran for CARGO={cargo_env_var:?}: {error}")
                })
        };

        assert_eq!(run_fake_cargo(None), "path: --color=never tree");
        assert_eq!(
            run_fake_cargo(Some(env_var_dir.join("cargo").into_os_string())),
            "env-var: --color=never tree",
        );
        assert_eq!(
            run_fake_cargo(Some(OsString::from("cargo-custom"))),
            "custom: --color=never tree",
        );

        // Don't run this subtest on Apple platforms since APFS rejects
        // non-UTF-8 names. (This is a rough heuristic in lieu of actually
        // checking whether the filesystem supports non-UTF-8 names.)
        #[cfg(not(target_vendor = "apple"))]
        {
            let non_utf8_dir = temp_dir
                .path()
                .join(OsString::from_vec(b"non-utf8-\xff".to_vec()));
            fs::create_dir(&non_utf8_dir).expect("created non-UTF-8 CARGO dir");
            write_fake_cargo(&non_utf8_dir.join("cargo"), "non-utf8");
            assert_eq!(
                run_fake_cargo(Some(non_utf8_dir.join("cargo").into_os_string())),
                "non-utf8: --color=never tree",
            );
        }
    }
}
