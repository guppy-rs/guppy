// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::output::{OutputContext, Styles};
use camino::Utf8PathBuf;
use hakari::summaries::HakariConfig;
use indenter::{Format, indented};
use log::error;
use owo_colors::OwoColorize;
use std::{
    error::Error,
    fmt::{self, Write as _},
    io,
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
            | Self::LockfileUpdateFailed { .. } => ERROR_EXIT_CODE,
        }
    }

    pub fn display_to_stderr(&self, output: &OutputContext) {
        error!("{}", self.report(&output.styles));
    }

    fn report<'a>(&'a self, styles: &'a Styles) -> ErrorReport<'a> {
        ErrorReport {
            error: self,
            styles,
        }
    }
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
        }

        write_causes(f, self.error.source())
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
    use crate::test_helpers::exit_status;
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

    fn config_not_found() -> ExpectedError {
        ExpectedError::ConfigNotFound {
            paths_tried: vec![
                "/workspace/.config/hakari.toml".into(),
                "/workspace/.guppy/hakari.toml".into(),
            ],
        }
    }

    fn config_read_failed(error: io::Error) -> ExpectedError {
        ExpectedError::ConfigReadFailed {
            config_path: "/workspace/.config/hakari.toml".into(),
            error,
        }
    }

    fn lockfile_update_failed() -> ExpectedError {
        ExpectedError::LockfileUpdateFailed {
            command: "/opt/rust/bin/cargo tree".to_owned(),
            exit_status: exit_status(101),
        }
    }

    #[test]
    fn report_config_not_found() {
        let error = config_not_found();
        assert_report_snapshot(&error, file!["snapshots/errors/config_not_found.txt"]);
        assert_eq!(
            error.to_string(),
            "no Hakari config found at any of these paths: \
             /workspace/.config/hakari.toml, /workspace/.guppy/hakari.toml",
        );
        assert_eq!(error.process_exit_code(), 1);
    }

    #[test]
    fn report_config_read_failed() {
        let error = config_read_failed(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied",
        ));
        assert_report_snapshot(&error, file!["snapshots/errors/config_read_failed.txt"]);
        assert_eq!(error.process_exit_code(), 1);
    }

    #[test]
    fn report_config_deserialize_failed() {
        // In these TOML contents, the string on line 2 is not closed. It spans
        // several lines and ends with a newline.
        let toml_error = "hakari-package = \"workspace-hack\"\nresolver = \"2\n"
            .parse::<HakariConfig>()
            .expect_err("an unterminated string is rejected");
        let error = ExpectedError::ConfigDeserializeFailed {
            config_path: "/workspace/.config/hakari.toml".into(),
            error: toml_error,
        };
        assert_report_snapshot(
            &error,
            file!["snapshots/errors/config_deserialize_failed.txt"],
        );
        assert_eq!(
            error.to_string(),
            "failed to deserialize Hakari config at /workspace/.config/hakari.toml",
        );
        assert_eq!(error.process_exit_code(), 1);
    }

    #[test]
    fn report_config_resolve_failed() {
        let error = ExpectedError::ConfigResolveFailed {
            config_path: "/workspace/.config/hakari.toml".into(),
            error: Box::new(guppy::Error::UnknownWorkspaceName("nope".to_owned())),
        };
        assert_report_snapshot(&error, file!["snapshots/errors/config_resolve_failed.txt"]);
        assert_eq!(
            error.to_string(),
            "failed to resolve Hakari config at /workspace/.config/hakari.toml",
        );
        assert_eq!(error.process_exit_code(), 1);
    }

    #[test]
    fn report_hakari_package_not_set() {
        let error = ExpectedError::HakariPackageNotSet {
            config_path: "/workspace/.config/hakari.toml".into(),
        };
        assert_report_snapshot(&error, file!["snapshots/errors/hakari_package_not_set.txt"]);
        assert_eq!(
            error.to_string(),
            "`hakari-package` is not set in /workspace/.config/hakari.toml, \
             so cargo hakari can't tell which crate is the workspace-hack",
        );
        assert_eq!(error.process_exit_code(), 1);
    }

    #[test]
    fn report_lockfile_update_exec_failed() {
        let error = ExpectedError::LockfileUpdateExecFailed {
            command: "/opt/rust/bin/cargo tree".to_owned(),
            error: io::Error::new(io::ErrorKind::NotFound, "program not found"),
        };
        assert_report_snapshot(
            &error,
            file!["snapshots/errors/lockfile_update_exec_failed.txt"],
        );
        assert_eq!(error.process_exit_code(), 1);
    }

    // (This is Unix-only because the exit status is reported slightly
    // differently on Windows.)
    #[cfg(unix)]
    #[test]
    fn report_lockfile_update_failed() {
        assert_report_snapshot(
            &lockfile_update_failed(),
            file!["snapshots/errors/lockfile_update_failed.txt"],
        );
    }

    #[test]
    fn lockfile_update_failed_exit_code() {
        assert_eq!(lockfile_update_failed().process_exit_code(), 1);
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
