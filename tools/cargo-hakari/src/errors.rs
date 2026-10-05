// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::output::{OutputContext, Styles};
use camino::Utf8PathBuf;
use hakari::{
    CargoTomlError, TomlOutError,
    cli_ops::{self, ApplyError},
    summaries::HakariConfig,
};
use indenter::{Format, indented};
use owo_colors::OwoColorize;
use std::{
    error::Error,
    fmt::{self, Write as _},
    io,
    path::PathBuf,
    process::ExitStatus,
    str::FromStr,
};
use thiserror::Error;

pub(crate) type Result<T, E = ExpectedError> = std::result::Result<T, E>;

/// The exit code for every error.
///
/// This is the same as `eyre`'s exit code.
const ERROR_EXIT_CODE: i32 = 1;

/// An expected (operational) cargo-hakari error.
///
/// The `#[error]` strings are one-line fallbacks -- `display_to_stderr` is the
/// intended rendering.
#[derive(Debug, Error)]
#[doc(hidden)]
pub enum ExpectedError {
    #[error(
        "no Hakari config found at any of these paths: {}",
        join_paths(.paths_tried)
    )]
    ConfigNotFound {
        /// The absolute paths, in the order they were tried.
        paths_tried: Vec<Utf8PathBuf>,
    },
    #[error("failed to read Hakari config at {config_path}")]
    ConfigReadFailed {
        config_path: Utf8PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("failed to deserialize Hakari config at {config_path}")]
    ConfigDeserializeFailed {
        config_path: Utf8PathBuf,
        #[source]
        error: <HakariConfig as FromStr>::Err,
    },
    #[error("failed to resolve Hakari config at {config_path}")]
    ConfigResolveFailed {
        config_path: Utf8PathBuf,
        #[source]
        error: Box<guppy::Error>,
    },
    #[error(
        "`hakari-package` is not set in {config_path}, so cargo hakari can't \
         tell which crate is the workspace-hack"
    )]
    HakariPackageNotSet { config_path: Utf8PathBuf },
    #[error("failed to update Cargo.lock: could not run `{command}`")]
    LockfileUpdateExecFailed {
        /// The full command line (see `CargoCli::display_command`).
        command: String,
        /// The underlying error.
        #[source]
        error: io::Error,
    },
    #[error("failed to update Cargo.lock: `{command}` failed with {exit_status}")]
    LockfileUpdateFailed {
        /// The full command line (see `CargoCli::display_command`).
        command: String,
        /// The exit status (this is never a success).
        exit_status: ExitStatus,
    },
    /// A package name given on the command line could not be resolved to a
    /// workspace member.
    ///
    /// Transparent because guppy's message already names the package.
    #[error(transparent)]
    WorkspacePackageResolveFailed {
        /// The underlying error.
        error: guppy::Error,
    },
    /// `cargo metadata` could not be run, or its output could not be turned
    /// into a package graph.
    #[error("failed to build package graph")]
    PackageGraphBuildFailed {
        /// The underlying error.
        #[source]
        error: guppy::Error,
    },
    /// `cargo hakari init` failed.
    #[error(transparent)]
    InitFailed { error: InitError },
    /// Applying changes to the workspace during `cargo hakari init` failed.
    ///
    /// The steps are not rolled back, so the workspace might be in an
    /// intermediate state.
    #[error("failed to initialize workspace-hack crate {package_name}")]
    InitApplyFailed {
        package_name: String,
        #[source]
        error: ApplyError,
    },
    /// Applying changes to the workspace during `cargo hakari manage-deps` failed.
    ///
    /// The steps are not rolled back.
    #[error("failed to update dependencies on {hakari_package_name} in workspace crates")]
    ManageDepsApplyFailed {
        hakari_package_name: String,
        #[source]
        error: ApplyError,
    },
    /// Applying changes to the workspace during `cargo hakari remove-deps` failed.
    ///
    /// The steps are not rolled back.
    #[error("failed to remove dependencies on {hakari_package_name} from workspace crates")]
    RemoveDepsApplyFailed {
        hakari_package_name: String,
        #[source]
        error: ApplyError,
    },
    /// The "proceed?" prompt could not be shown or answered, for example
    /// because stderr is not a terminal.
    #[error("failed to read confirmation")]
    ConfirmReadFailed {
        #[source]
        error: dialoguer::Error,
    },
    /// `cargo hakari generate` or `cargo hakari disable` failed to update the
    /// workspace-hack's `Cargo.toml`.
    #[error(transparent)]
    HakariCargoTomlUpdateFailed { error: HakariCargoTomlUpdateError },
    #[error(
        "failed to remove the dependency on {hakari_package_name} from \
         {package_name} before publishing"
    )]
    PublishDepRemoveFailed {
        /// The package being published and whose manifest was being edited.
        package_name: String,
        hakari_package_name: String,
        #[source]
        error: ApplyError,
    },
    #[error(
        "failed to re-add the dependency on {hakari_package_name} to \
         {package_name} after removing it for publishing"
    )]
    PublishDepRestoreFailed {
        /// The package being published and whose manifest was being edited.
        package_name: String,
        hakari_package_name: String,
        #[source]
        error: ApplyError,
    },
    #[error("failed to publish {package_name}: could not run `{command}`")]
    PublishExecFailed {
        package_name: String,
        /// The full command line (see `CargoCli::display_command`).
        command: String,
        /// The underlying error.
        #[source]
        error: io::Error,
    },
    #[error("failed to publish {package_name}: `{command}` failed with {exit_status}")]
    PublishFailed {
        package_name: String,
        /// The full command line (see `CargoCli::display_command`).
        command: String,
        /// The exit status (this is never a success).
        exit_status: ExitStatus,
    },
}

impl ExpectedError {
    pub fn process_exit_code(&self) -> i32 {
        match self {
            Self::ConfigNotFound { .. }
            | Self::ConfigReadFailed { .. }
            | Self::ConfigDeserializeFailed { .. }
            | Self::ConfigResolveFailed { .. }
            | Self::HakariPackageNotSet { .. }
            | Self::LockfileUpdateExecFailed { .. }
            | Self::LockfileUpdateFailed { .. }
            | Self::WorkspacePackageResolveFailed { .. }
            | Self::PackageGraphBuildFailed { .. }
            | Self::InitFailed { .. }
            | Self::InitApplyFailed { .. }
            | Self::ManageDepsApplyFailed { .. }
            | Self::RemoveDepsApplyFailed { .. }
            | Self::ConfirmReadFailed { .. }
            | Self::HakariCargoTomlUpdateFailed { .. }
            | Self::PublishDepRemoveFailed { .. }
            | Self::PublishDepRestoreFailed { .. }
            | Self::PublishExecFailed { .. }
            | Self::PublishFailed { .. } => ERROR_EXIT_CODE,
        }
    }

    /// Displays the error to stderr.
    ///
    /// This is not subject to `RUST_LOG` and is always displayed.
    pub fn display_to_stderr(&self, output: &OutputContext) {
        output.display_fatal_error(self.report(&output.styles));
    }

    fn report<'a>(&'a self, styles: &'a Styles) -> ErrorReport<'a> {
        ErrorReport {
            error: self,
            styles,
        }
    }
}

/// The reason why `cargo hakari init` failed.
///
/// Part of [`ExpectedError::InitFailed`]. Not to be confused with
/// [`hakari::cli_ops::InitError`].
#[derive(Debug, Error)]
pub enum InitError {
    /// No package name was provided and the path doesn't end in a filename.
    #[error(
        "path '{crate_path}' doesn't end in a directory name, so cargo hakari \
         can't derive a package name from it"
    )]
    PathMissingFileName {
        /// The path as provided on the command line.
        crate_path: Utf8PathBuf,
    },
    /// Accessing the current directory failed.
    #[error("failed to access the current directory")]
    CurrentDirAccessFailed {
        /// The underlying error.s
        #[source]
        error: io::Error,
    },
    /// The current directory was invalid UTF-8.
    #[error("current directory {} is not valid UTF-8", .current_dir.display())]
    CurrentDirNotUtf8 {
        /// The current directory path.
        current_dir: PathBuf,
    },
    /// The provided path is outside the workspace.
    #[error("path {crate_path} is not inside workspace root {workspace_root}")]
    PathOutsideWorkspace {
        /// The absolute version of the path as provided on the command line.
        crate_path: Utf8PathBuf,
        workspace_root: Utf8PathBuf,
    },
    /// Hakari failed to initialize a workspace-hack crate.
    #[error(transparent)]
    PreconditionFailed { error: cli_ops::InitError },
}

/// Why `cargo hakari generate` or `cargo hakari disable` failed.
///
/// Part of [`ExpectedError::HakariCargoTomlUpdateFailed`].
#[derive(Debug, Error)]
pub enum HakariCargoTomlUpdateError {
    #[error("failed to generate new contents for {hakari_package_name}")]
    ContentsGenerate {
        hakari_package_name: String,
        #[source]
        error: TomlOutError,
    },
    #[error(
        "failed to read the generated section of Cargo.toml for \
         {hakari_package_name}"
    )]
    Read {
        hakari_package_name: String,
        #[source]
        error: CargoTomlError,
    },
    #[error("failed to write updated contents to Cargo.toml for {hakari_package_name}")]
    Write {
        hakari_package_name: String,
        #[source]
        error: CargoTomlError,
    },
}

#[derive(Clone, Copy, Debug)]
struct ErrorReport<'a> {
    error: &'a ExpectedError,
    styles: &'a Styles,
}

impl fmt::Display for ErrorReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let styles = self.styles;

        // The output doesn't have a trailing newline because the logger adds
        // one.
        match self.error {
            ExpectedError::ConfigNotFound { paths_tried } => {
                f.write_str("no Hakari config found at any of these paths:")?;
                for path in paths_tried {
                    write!(f, "\n  - {}", path.style(styles.config_path))?;
                }
            }
            ExpectedError::ConfigReadFailed {
                config_path,
                error: _,
            } => {
                write!(
                    f,
                    "failed to read Hakari config at {}",
                    config_path.style(styles.config_path),
                )?;
            }
            ExpectedError::ConfigDeserializeFailed {
                config_path,
                error: _,
            } => {
                write!(
                    f,
                    "failed to deserialize Hakari config at {}",
                    config_path.style(styles.config_path),
                )?;
            }
            ExpectedError::ConfigResolveFailed {
                config_path,
                error: _,
            } => {
                write!(
                    f,
                    "failed to resolve Hakari config at {}",
                    config_path.style(styles.config_path),
                )?;
            }
            ExpectedError::HakariPackageNotSet { config_path } => {
                write!(
                    f,
                    "`hakari-package` is not set in {}, so cargo hakari can't \
                     tell which crate is the workspace-hack",
                    config_path.style(styles.config_path),
                )?;
                write_hint(
                    f,
                    "set `hakari-package` to that crate's name, for example \
                     `hakari-package = \"workspace-hack\"`",
                )?;
                write_hint(
                    f,
                    "if that crate doesn't exist yet, run \
                     `cargo hakari init --skip-config <path>` first",
                )?;
            }
            ExpectedError::LockfileUpdateExecFailed { command, error: _ } => {
                write!(
                    f,
                    "failed to update Cargo.lock: could not run `{}`",
                    command.style(styles.command),
                )?;
            }
            ExpectedError::LockfileUpdateFailed {
                command,
                exit_status,
            } => {
                write!(
                    f,
                    "failed to update Cargo.lock: `{}` failed with {exit_status}",
                    command.style(styles.command),
                )?;
            }
            ExpectedError::WorkspacePackageResolveFailed { error } => {
                write!(f, "{error}")?;
            }
            ExpectedError::PackageGraphBuildFailed { error: _ } => {
                f.write_str("failed to build package graph")?;
            }
            ExpectedError::InitFailed { error } => {
                write_init_error(f, error)?;
            }
            ExpectedError::InitApplyFailed {
                package_name,
                error: _,
            } => {
                write!(
                    f,
                    "failed to initialize workspace-hack crate {}",
                    package_name.style(styles.package_name),
                )?;
            }
            ExpectedError::ManageDepsApplyFailed {
                hakari_package_name,
                error: _,
            } => {
                write!(
                    f,
                    "failed to update dependencies on {} in workspace crates",
                    hakari_package_name.style(styles.package_name),
                )?;
            }
            ExpectedError::RemoveDepsApplyFailed {
                hakari_package_name,
                error: _,
            } => {
                write!(
                    f,
                    "failed to remove dependencies on {} from workspace crates",
                    hakari_package_name.style(styles.package_name),
                )?;
            }
            ExpectedError::ConfirmReadFailed { error: _ } => {
                f.write_str("failed to read confirmation")?;
                write_hint(
                    f,
                    "pass `--yes` to proceed without confirmation, or \
                     `--dry-run` to only print the operations",
                )?;
            }
            ExpectedError::HakariCargoTomlUpdateFailed { error } => {
                write_hakari_cargo_toml_update_error(f, error, styles)?;
            }
            ExpectedError::PublishDepRemoveFailed {
                package_name,
                hakari_package_name,
                error: _,
            } => {
                write!(
                    f,
                    "failed to remove the dependency on {} from {} before \
                     publishing",
                    hakari_package_name.style(styles.package_name),
                    package_name.style(styles.package_name),
                )?;
            }
            ExpectedError::PublishDepRestoreFailed {
                package_name,
                hakari_package_name,
                error: _,
            } => {
                write!(
                    f,
                    "failed to re-add the dependency on {} to {} after \
                     removing it for publishing",
                    hakari_package_name.style(styles.package_name),
                    package_name.style(styles.package_name),
                )?;

                // manage-deps only adds the dependency to crates the config
                // doesn't exclude, while publish removes and re-adds it for any
                // that has it -- hence the "unless the Hakari config excludes
                // ...".
                write_hint(
                    f,
                    format_args!(
                        "after fixing the cause, run \
                         `cargo hakari manage-deps -p {}` to add it back, \
                         unless the Hakari config excludes {}",
                        package_name.style(styles.package_name),
                        package_name.style(styles.package_name),
                    ),
                )?;
            }
            ExpectedError::PublishExecFailed {
                package_name,
                command,
                error: _,
            } => {
                write!(
                    f,
                    "failed to publish {}: could not run `{}`",
                    package_name.style(styles.package_name),
                    command.style(styles.command),
                )?;
            }
            ExpectedError::PublishFailed {
                package_name,
                command,
                exit_status,
            } => {
                write!(
                    f,
                    "failed to publish {}: `{}` failed with {exit_status}",
                    package_name.style(styles.package_name),
                    command.style(styles.command),
                )?;
            }
        }

        write_causes(f, self.error.source())
    }
}

fn write_init_error(f: &mut fmt::Formatter<'_>, error: &InitError) -> fmt::Result {
    match error {
        InitError::PathMissingFileName { crate_path } => {
            write!(
                f,
                "path '{crate_path}' doesn't end in a directory name, so \
                 cargo hakari can't derive a package name from it",
            )
        }
        InitError::CurrentDirAccessFailed { error: _ } => {
            f.write_str("failed to access the current directory")
        }
        InitError::CurrentDirNotUtf8 { current_dir } => {
            write!(
                f,
                "current directory {} is invalid UTF-8",
                current_dir.display(),
            )
        }
        InitError::PathOutsideWorkspace {
            crate_path,
            workspace_root,
        } => {
            write!(
                f,
                "path {crate_path} is not inside workspace root \
                 {workspace_root}",
            )
        }
        InitError::PreconditionFailed { error } => {
            write!(f, "{error}")
        }
    }
}

fn write_hakari_cargo_toml_update_error(
    f: &mut fmt::Formatter<'_>,
    error: &HakariCargoTomlUpdateError,
    styles: &Styles,
) -> fmt::Result {
    match error {
        HakariCargoTomlUpdateError::ContentsGenerate {
            hakari_package_name,
            error: _,
        } => {
            write!(
                f,
                "failed to generate new contents for {}",
                hakari_package_name.style(styles.package_name),
            )
        }
        HakariCargoTomlUpdateError::Read {
            hakari_package_name,
            error: _,
        } => {
            write!(
                f,
                "failed to read the generated section of Cargo.toml for {}",
                hakari_package_name.style(styles.package_name),
            )
        }
        HakariCargoTomlUpdateError::Write {
            hakari_package_name,
            error: _,
        } => {
            write!(
                f,
                "failed to write updated contents to Cargo.toml for {}",
                hakari_package_name.style(styles.package_name),
            )
        }
    }
}

fn write_hint(f: &mut fmt::Formatter<'_>, hint: impl fmt::Display) -> fmt::Result {
    write!(f, "\n(hint: {hint})")
}

// (Ported from DisplayErrorChain in nextest-runner.)
fn write_causes(
    f: &mut fmt::Formatter<'_>,
    first_cause: Option<&(dyn Error + 'static)>,
) -> fmt::Result {
    let Some(mut cause) = first_cause else {
        return Ok(());
    };

    f.write_str("\n  caused by:")?;
    loop {
        f.write_str("\n")?;
        let mut indent_continuation_lines = |line: usize, out: &mut dyn fmt::Write| match line {
            0 => Ok(()),
            _ => out.write_str("    "),
        };
        let mut indent = indented(f).with_format(Format::Custom {
            inserter: &mut indent_continuation_lines,
        });
        write!(indent, "  - {cause}")?;

        let Some(next_cause) = cause.source() else {
            return Ok(());
        };
        cause = next_cause;
    }
}

fn join_paths(paths: &[Utf8PathBuf]) -> String {
    let paths: Vec<&str> = paths.iter().map(|path| path.as_str()).collect();
    paths.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_helpers::{exit_status, non_utf8_path, reverse_dep_builder};
    use guppy::PackageId;
    use snapbox::{Data, assert_data_eq, file};

    #[derive(Debug, Error)]
    #[error("{message}")]
    struct TestCause {
        message: &'static str,
        #[source]
        source: Option<Box<TestCause>>,
    }

    impl TestCause {
        fn io_error_chain(messages: &[&'static str]) -> io::Error {
            let mut chain: Option<Box<TestCause>> = None;
            for &message in messages.iter().rev() {
                chain = Some(Box::new(TestCause {
                    message,
                    source: chain,
                }));
            }
            io::Error::other(chain.expect("at least one message was passed in"))
        }
    }

    #[track_caller]
    fn assert_report_snapshot(error: &ExpectedError, expected: Data) {
        let actual = format!("{}\n", error.report(&Styles::default()));
        assert!(
            !actual.contains('\r'),
            "report has no carriage returns: {actual:?}",
        );
        assert_data_eq!(actual, expected.raw());
    }

    struct Example {
        error: ExpectedError,
        // What display_to_stderr writes after the `error:` prefix.
        report: Data,
        // The Display output (one-line fallback).
        one_line: &'static str,
        exit_code: i32,
    }

    impl Example {
        fn assert(self) {
            let Self {
                error,
                report,
                one_line,
                exit_code,
            } = self;
            assert_report_snapshot(&error, report);
            assert_eq!(error.to_string(), one_line);
            assert_eq!(
                error.process_exit_code(),
                exit_code,
                "exit code of {error:?}",
            );
        }
    }

    fn config_read_failed(error: io::Error) -> ExpectedError {
        ExpectedError::ConfigReadFailed {
            config_path: "/workspace/.config/hakari.toml".into(),
            error,
        }
    }

    /// Synthesizes an [`ApplyError`] for use in tests.
    #[cfg(unix)]
    fn apply_error() -> ApplyError {
        let builder = reverse_dep_builder();
        let workspace = builder.graph().resolve_workspace();
        builder
            .remove_dep_ops(&workspace, false)
            .apply()
            .expect_err("the fixture's workspace root doesn't exist")
    }

    fn examples() -> Vec<Example> {
        vec![
            Example {
                error: ExpectedError::ConfigNotFound {
                    paths_tried: vec![
                        "/workspace/.config/hakari.toml".into(),
                        "/workspace/.guppy/hakari.toml".into(),
                    ],
                },
                report: file!["snapshots/errors/config_not_found.txt"],
                one_line: "no Hakari config found at any of these paths: \
                           /workspace/.config/hakari.toml, /workspace/.guppy/hakari.toml",
                exit_code: 1,
            },
            Example {
                error: config_read_failed(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "permission denied",
                )),
                report: file!["snapshots/errors/config_read_failed.txt"],
                one_line: "failed to read Hakari config at /workspace/.config/hakari.toml",
                exit_code: 1,
            },
            Example {
                // In these TOML contents, the string on line 2 is not closed.
                // It spans several lines and ends with a newline.
                error: ExpectedError::ConfigDeserializeFailed {
                    config_path: "/workspace/.config/hakari.toml".into(),
                    error: "hakari-package = \"workspace-hack\"\nresolver = \"2\n"
                        .parse::<HakariConfig>()
                        .expect_err("an unterminated string is rejected"),
                },
                report: file!["snapshots/errors/config_deserialize_failed.txt"],
                one_line: "failed to deserialize Hakari config at \
                           /workspace/.config/hakari.toml",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::ConfigResolveFailed {
                    config_path: "/workspace/.config/hakari.toml".into(),
                    error: Box::new(guppy::Error::UnknownWorkspaceName("nope".to_owned())),
                },
                report: file!["snapshots/errors/config_resolve_failed.txt"],
                one_line: "failed to resolve Hakari config at /workspace/.config/hakari.toml",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::HakariPackageNotSet {
                    config_path: "/workspace/.config/hakari.toml".into(),
                },
                report: file!["snapshots/errors/hakari_package_not_set.txt"],
                one_line: "`hakari-package` is not set in /workspace/.config/hakari.toml, \
                           so cargo hakari can't tell which crate is the workspace-hack",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::LockfileUpdateExecFailed {
                    command: "/opt/rust/bin/cargo tree".to_owned(),
                    error: io::Error::new(io::ErrorKind::NotFound, "program not found"),
                },
                report: file!["snapshots/errors/lockfile_update_exec_failed.txt"],
                one_line: "failed to update Cargo.lock: could not run `/opt/rust/bin/cargo tree`",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::WorkspacePackageResolveFailed {
                    error: guppy::Error::UnknownWorkspaceName("nope".to_owned()),
                },
                report: file!["snapshots/errors/workspace_package_resolve_failed.txt"],
                one_line: "unknown workspace package name: nope",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::PackageGraphBuildFailed {
                    error: guppy::Error::CommandError(Box::new(io::Error::new(
                        io::ErrorKind::NotFound,
                        "program not found",
                    ))),
                },
                report: file!["snapshots/errors/package_graph_build_failed.txt"],
                one_line: "failed to build package graph",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitFailed {
                    error: InitError::PathMissingFileName {
                        crate_path: "crates/..".into(),
                    },
                },
                report: file!["snapshots/errors/init_path_missing_file_name.txt"],
                one_line: "path 'crates/..' doesn't end in a directory name, so cargo hakari \
                           can't derive a package name from it",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitFailed {
                    error: InitError::CurrentDirAccessFailed {
                        error: io::Error::new(io::ErrorKind::NotFound, "directory not found"),
                    },
                },
                report: file!["snapshots/errors/current_dir_access_failed.txt"],
                one_line: "failed to access the current directory",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitFailed {
                    error: InitError::PathOutsideWorkspace {
                        crate_path: "/elsewhere/workspace-hack".into(),
                        workspace_root: "/workspace".into(),
                    },
                },
                report: file!["snapshots/errors/init_path_outside_workspace.txt"],
                one_line: "path /elsewhere/workspace-hack is not inside workspace root /workspace",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitFailed {
                    error: InitError::PreconditionFailed {
                        error: cli_ops::InitError::Io {
                            path: "/workspace/workspace-hack".into(),
                            error: io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "permission denied",
                            ),
                        },
                    },
                },
                report: file!["snapshots/errors/init_precondition_failed.txt"],
                one_line: "IO error while accessing /workspace/workspace-hack",
                exit_code: 1,
            },
            Example {
                // Dialoguer returns this error when stderr is not a terminal.
                error: ExpectedError::ConfirmReadFailed {
                    error: dialoguer::Error::IO(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "not a terminal",
                    )),
                },
                report: file!["snapshots/errors/confirm_read_failed.txt"],
                one_line: "failed to read confirmation",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::HakariCargoTomlUpdateFailed {
                    error: HakariCargoTomlUpdateError::ContentsGenerate {
                        hakari_package_name: "my-workspace-hack".to_owned(),
                        error: TomlOutError::UnrecognizedExternal {
                            package_id: PackageId::new("foo 1.2.3 (svn+https://example.com/foo)"),
                            source: "svn+https://example.com/foo".to_owned(),
                        },
                    },
                },
                report: file!["snapshots/errors/hakari_contents_generate_failed.txt"],
                one_line: "failed to generate new contents for my-workspace-hack",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::HakariCargoTomlUpdateFailed {
                    error: HakariCargoTomlUpdateError::Read {
                        hakari_package_name: "my-workspace-hack".to_owned(),
                        error: CargoTomlError::GeneratedSectionNotFound {
                            toml_path: "/workspace/my-workspace-hack/Cargo.toml".into(),
                        },
                    },
                },
                report: file!["snapshots/errors/hakari_cargo_toml_read_failed.txt"],
                one_line: "failed to read the generated section of Cargo.toml for \
                           my-workspace-hack",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::HakariCargoTomlUpdateFailed {
                    error: HakariCargoTomlUpdateError::Write {
                        hakari_package_name: "my-workspace-hack".to_owned(),
                        error: CargoTomlError::Io {
                            toml_path: "/workspace/my-workspace-hack/Cargo.toml".into(),
                            error: io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "permission denied",
                            ),
                        },
                    },
                },
                report: file!["snapshots/errors/hakari_cargo_toml_write_failed.txt"],
                one_line: "failed to write updated contents to Cargo.toml for my-workspace-hack",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::PublishExecFailed {
                    package_name: "hrd-member-normal".to_owned(),
                    command: "/opt/rust/bin/cargo publish --dry-run --allow-dirty".to_owned(),
                    error: io::Error::new(io::ErrorKind::NotFound, "program not found"),
                },
                report: file!["snapshots/errors/publish_exec_failed.txt"],
                one_line: "failed to publish hrd-member-normal: could not run \
                           `/opt/rust/bin/cargo publish --dry-run --allow-dirty`",
                exit_code: 1,
            },
        ]
    }

    #[cfg(unix)]
    fn unix_examples() -> Vec<Example> {
        vec![
            Example {
                error: ExpectedError::LockfileUpdateFailed {
                    command: "/opt/rust/bin/cargo tree".to_owned(),
                    exit_status: exit_status(101),
                },
                report: file!["snapshots/errors/lockfile_update_failed.txt"],
                one_line: "failed to update Cargo.lock: `/opt/rust/bin/cargo tree` failed with \
                           exit status: 101",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitFailed {
                    error: InitError::CurrentDirNotUtf8 {
                        current_dir: non_utf8_path(),
                    },
                },
                report: file!["snapshots/errors/current_dir_not_utf8.txt"],
                one_line: "current directory /workspace/\u{fffd} is not valid UTF-8",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::InitApplyFailed {
                    package_name: "my-workspace-hack".to_owned(),
                    error: apply_error(),
                },
                report: file!["snapshots/errors/init_apply_failed.txt"],
                one_line: "failed to initialize workspace-hack crate my-workspace-hack",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::ManageDepsApplyFailed {
                    hakari_package_name: "hrd-workspace-hack".to_owned(),
                    error: apply_error(),
                },
                report: file!["snapshots/errors/manage_deps_apply_failed.txt"],
                one_line: "failed to update dependencies on hrd-workspace-hack in workspace crates",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::RemoveDepsApplyFailed {
                    hakari_package_name: "hrd-workspace-hack".to_owned(),
                    error: apply_error(),
                },
                report: file!["snapshots/errors/remove_deps_apply_failed.txt"],
                one_line: "failed to remove dependencies on hrd-workspace-hack from \
                           workspace crates",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::PublishDepRemoveFailed {
                    package_name: "hrd-member-normal".to_owned(),
                    hakari_package_name: "hrd-workspace-hack".to_owned(),
                    error: apply_error(),
                },
                report: file!["snapshots/errors/publish_dep_remove_failed.txt"],
                one_line: "failed to remove the dependency on hrd-workspace-hack from \
                           hrd-member-normal before publishing",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::PublishDepRestoreFailed {
                    package_name: "hrd-member-normal".to_owned(),
                    hakari_package_name: "hrd-workspace-hack".to_owned(),
                    error: apply_error(),
                },
                report: file!["snapshots/errors/publish_dep_restore_failed.txt"],
                one_line: "failed to re-add the dependency on hrd-workspace-hack to \
                           hrd-member-normal after removing it for publishing",
                exit_code: 1,
            },
            Example {
                error: ExpectedError::PublishFailed {
                    package_name: "hrd-member-normal".to_owned(),
                    command: "/opt/rust/bin/cargo publish --dry-run --allow-dirty".to_owned(),
                    exit_status: exit_status(101),
                },
                report: file!["snapshots/errors/publish_failed.txt"],
                one_line: "failed to publish hrd-member-normal: \
                           `/opt/rust/bin/cargo publish --dry-run --allow-dirty` failed \
                           with exit status: 101",
                exit_code: 1,
            },
        ]
    }

    #[test]
    fn report_examples() {
        for example in examples() {
            example.assert();
        }
    }

    #[cfg(unix)]
    #[test]
    fn report_unix_examples() {
        for example in unix_examples() {
            example.assert();
        }
    }

    #[test]
    fn report_cause_chain() {
        let error = config_read_failed(TestCause::io_error_chain(&[
            "outermost cause",
            "middle cause",
            "innermost cause",
        ]));
        assert_report_snapshot(&error, file!["snapshots/errors/cause_chain.txt"]);
    }

    #[test]
    fn report_multi_line_causes() {
        let error = config_read_failed(TestCause::io_error_chain(&[
            // A message shaped like a TOML parse error, which ends with a
            // newline.
            concat!(
                "TOML parse error at line 2, column 12\n",
                "  |\n",
                "2 | resolver = 2\n",
                "  |            ^\n",
                "invalid type: integer `2`, expected a string\n",
            ),
            // A blank line in the middle.
            "first paragraph\n\nsecond paragraph",
            "single-line cause",
        ]));
        assert_report_snapshot(&error, file!["snapshots/errors/multi_line_causes.txt"]);
    }
}
