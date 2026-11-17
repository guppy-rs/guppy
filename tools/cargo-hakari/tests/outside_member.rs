// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test workspace members stored outside the workspace root.
//!
//! A `members` entry may begin with `..`, placing the member's directory beside the workspace
//! root rather than under it, which Cargo accepts when the member names its root through
//! `package.workspace`. `manage-deps` has to write that member's dependency on the
//! workspace-hack crate like any other.

use snapbox::cmd::{Command, cargo_bin};
use std::fs;
use tempfile::TempDir;

/// Files for a workspace at `root/` with one member under it and one member beside it.
const WORKSPACE_FILES: [(&str, &str); 8] = [
    (
        "root/Cargo.toml",
        "[workspace]\nmembers = [\"inside\", \"../outside\", \"workspace-hack\"]\nresolver = \"2\"\n",
    ),
    (
        "root/.config/hakari.toml",
        "hakari-package = \"workspace-hack\"\ndep-format-version = \"4\"\nresolver = \"2\"\n",
    ),
    (
        "root/workspace-hack/Cargo.toml",
        "[package]\nname = \"workspace-hack\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n",
    ),
    ("root/workspace-hack/src/lib.rs", ""),
    (
        "root/inside/Cargo.toml",
        "[package]\nname = \"inside\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    ("root/inside/src/lib.rs", ""),
    (
        "outside/Cargo.toml",
        // Cargo accepts a member outside the root only when the member names the root itself.
        "[package]\nworkspace = \"../root\"\nname = \"outside\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    ("outside/src/lib.rs", ""),
];

/// Writes the files into a new temporary directory, creating each parent directory.
fn temp_tree(files: &[(&str, &str)]) -> TempDir {
    let tree = TempDir::new().expect("created temp dir");
    for (path, contents) in files {
        let path = tree.path().join(path);
        let parent = path.parent().expect("file path has a parent");
        fs::create_dir_all(parent).expect("created parent dir");
        fs::write(&path, contents).expect("wrote file");
    }
    tree
}

#[test]
fn manage_deps_writes_a_member_beside_the_workspace_root() {
    let tree = temp_tree(&WORKSPACE_FILES);

    Command::new(cargo_bin!("cargo-hakari"))
        .args(["hakari", "manage-deps", "--yes"])
        .current_dir(tree.path().join("root"))
        .env("CARGO_NET_OFFLINE", "true")
        .assert()
        .success();

    let inside = fs::read_to_string(tree.path().join("root/inside/Cargo.toml"))
        .expect("read the member under the workspace root");
    assert!(
        inside.contains(r#"path = "../workspace-hack""#),
        "the member under the root depends on the workspace-hack crate beside it, found:\n{inside}"
    );

    let outside = fs::read_to_string(tree.path().join("outside/Cargo.toml"))
        .expect("read the member beside the workspace root");
    assert!(
        outside.contains(r#"path = "../root/workspace-hack""#),
        "the member beside the root reaches the workspace-hack crate through the root, found:\n{outside}"
    );
}
