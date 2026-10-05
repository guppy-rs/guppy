// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    builder::{BuilderWithHakariPackage, builder_and_output_from_config},
    output::{Color, OutputContext, Styles},
};
use camino::Utf8Path;
use fixtures::json::JsonFixture;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
#[cfg(unix)]
use std::{
    ffi::OsStr,
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt, process::ExitStatusExt},
    path::Path,
    process::ExitStatus,
};
use std::{ffi::OsString, path::PathBuf, sync::Arc};

pub(crate) fn output_context() -> OutputContext {
    OutputContext {
        quiet: false,
        verbose: false,
        color: Color::Never,
        styles: Arc::new(Styles::default()),
    }
}

/// Returns a builder over the `hakari-reverse-dep` fixture, with
/// `hrd-workspace-hack` as the hakari package.
///
/// The fixture's workspace root (`/Users/fakeuser/...`) is not on disk, so
/// applying any workspace operation from this builder fails before a file is
/// touched.
pub(crate) fn reverse_dep_builder() -> BuilderWithHakariPackage<'static> {
    let (builder, _) = builder_and_output_from_config(
        JsonFixture::metadata_hakari_reverse_dep().graph(),
        Utf8Path::new("/workspace/.config/hakari.toml"),
        "hakari-package = \"hrd-workspace-hack\"\nresolver = \"2\"\n",
    )
    .expect("config with hakari-package resolves");
    builder
}

/// Returns the status of a process that exited with this code.
#[cfg(unix)]
pub(crate) fn exit_status(code: u8) -> ExitStatus {
    ExitStatus::from_raw(i32::from(code) << 8)
}

#[cfg(unix)]
pub(crate) fn write_executable_script(path: &Path, contents: &str) {
    fs::write(path, contents).expect("wrote script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("made script executable");
}

#[cfg(unix)]
pub(crate) fn non_utf8_path() -> PathBuf {
    // 0xff is invalid UTF-8.
    let path: OsString = OsStr::from_bytes(b"/workspace/\xff").to_owned();
    PathBuf::from(path)
}

#[cfg(windows)]
pub(crate) fn non_utf8_path() -> PathBuf {
    let mut wide: Vec<u16> = "C:\\workspace\\".encode_utf16().collect();
    // An unpaired surrogate is invalid UTF-8.
    wide.push(0xd800);
    PathBuf::from(OsString::from_wide(&wide))
}
