// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test error output.

use snapbox::{
    cmd::{Command, cargo_bin},
    str,
};
use std::fs;
use tempfile::TempDir;

/// The list of environment variables read by env_logger and supports-color.
///
/// `RUST_LOG` is not used for fatal errors, but it is used for info.
const OUTPUT_ENV_VARS: [&str; 7] = [
    "RUST_LOG",
    "RUST_LOG_STYLE",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "IGNORE_IS_TERMINAL",
];

const CARGO_TOML: (&str, &str) = (
    "Cargo.toml",
    // Here, "[workspace]" makes it a workspace root regardless of what the
    // parent has.
    "[package]\nname = \"e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
);
const LIB_RS: (&str, &str) = ("src/lib.rs", "");
const HAKARI_TOML: (&str, &str) = (
    ".config/hakari.toml",
    "hakari-package = \"e2e\"\nresolver = \"2\"\n",
);

fn temp_workspace(files: &[(&str, &str)]) -> TempDir {
    let workspace = TempDir::new().expect("created temp dir");
    for (path, contents) in files {
        let path = workspace.path().join(path);
        let parent = path.parent().expect("file path has a parent");
        fs::create_dir_all(parent).expect("created parent dir");
        fs::write(&path, contents).expect("wrote file");
    }
    workspace
}

fn cargo_hakari(workspace: &TempDir) -> Command {
    let mut command = Command::new(cargo_bin!("cargo-hakari"))
        .arg("hakari")
        .current_dir(workspace.path())
        .env("CARGO_NET_OFFLINE", "true");
    for var in OUTPUT_ENV_VARS {
        command = command.env_remove(var);
    }
    command
}

#[track_caller]
fn assert_unknown_package_error(command: Command) {
    command
        .args(["manage-deps", "--package", "nope"])
        .assert()
        .code(1)
        .stderr_eq(str![[r#"
error: unknown workspace package name: nope

"#]]);
}

#[test]
fn fatal_error_is_always_shown() {
    let workspace = temp_workspace(&[CARGO_TOML, LIB_RS, HAKARI_TOML]);

    assert_unknown_package_error(cargo_hakari(&workspace));
    assert_unknown_package_error(cargo_hakari(&workspace).arg("--quiet"));
    assert_unknown_package_error(cargo_hakari(&workspace).env("RUST_LOG", "cargo_hakari=off"));
    assert_unknown_package_error(
        cargo_hakari(&workspace).env("RUST_LOG", "/text-that-is-not-in-the-error"),
    );
}

#[test]
fn rust_log_still_filters_log_lines() {
    let workspace = temp_workspace(&[CARGO_TOML, LIB_RS, HAKARI_TOML]);

    // e2e is both the hakari package and the only workspace member, so there is
    // no crate to add a dependency to.
    cargo_hakari(&workspace)
        .arg("manage-deps")
        .assert()
        .code(0)
        .stderr_eq(str![[r#"
info: no operations to perform

"#]]);

    cargo_hakari(&workspace)
        .arg("manage-deps")
        .env("RUST_LOG", "cargo_hakari=off")
        .assert()
        .code(0)
        .stderr_eq(str![""]);
}

#[test]
fn fatal_error_follows_rust_log_style() {
    let workspace = temp_workspace(&[CARGO_TOML, LIB_RS, HAKARI_TOML]);

    cargo_hakari(&workspace)
        .args(["manage-deps", "--package", "nope", "--color=always"])
        .env("RUST_LOG_STYLE", "always")
        .assert()
        .code(1)
        .stderr_eq("\x1b[..]error:[..] unknown workspace package name: nope\n");

    assert_unknown_package_error(
        cargo_hakari(&workspace)
            .arg("--color=always")
            .env("RUST_LOG_STYLE", "never"),
    );
}

#[test]
fn init_without_terminal_to_confirm_on() {
    let workspace = temp_workspace(&[CARGO_TOML, LIB_RS]);

    // The cause itself is dialoguer's wording, so we don't assert on it here.
    cargo_hakari(&workspace)
        .args(["init", "workspace-hack"])
        .assert()
        .code(1)
        .stderr_eq(str![[r#"
info: operations to perform:
...
error: failed to read confirmation
(hint: pass `--yes` to proceed without confirmation, or `--dry-run` to only print the operations)
  caused by:
  - [..]

"#]]);
    assert!(
        !workspace.path().join("workspace-hack").exists(),
        "nothing is created without confirmation",
    );
}
