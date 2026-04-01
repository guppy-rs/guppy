// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parse custom platforms from `rustc --print=cfg` output.
//!
//! The `rustc --print=cfg` format is a line-oriented text format where
//! each line is either:
//!
//! - A key-value pair: `key="value"`
//! - A bare flag: `unix`, `debug_assertions`
//!
//! Bare flags fall into two categories:
//!
//! - Family aliases like `unix` and `windows`, which are
//!   redundant with `target_family="unix"` /
//!   `target_family="windows"` (cfg-expr matches `cfg(unix)`
//!   against the `families` field of `TargetInfo`, populated
//!   from `target_family` lines).
//! - Build configuration flags like `debug_assertions`, which
//!   describe the build profile rather than the target
//!   platform.
//!
//! Both categories are skipped during parsing.

use crate::errors::CustomTripleCreateError;
use cfg_expr::targets::{
    Abi, Arch, Endian, Env, Families, Family, HasAtomic, HasAtomics, Os, Panic, TargetInfo, Triple,
    Vendor,
};
use std::{borrow::Cow, collections::BTreeSet};

/// Parses `rustc --print=cfg` output into a `TargetInfo` and a set
/// of target features.
pub(crate) fn parse_cfg_output(
    triple: Cow<'static, str>,
    cfg_text: &str,
) -> Result<(TargetInfo, BTreeSet<String>), CustomTripleCreateError> {
    let parsed = ParsedCfg::parse(&triple, cfg_text)?;
    parsed.into_target_info_and_features(triple, cfg_text)
}

/// Tracks the parse state of a single-valued cfg key.
///
/// Distinguishes "not yet seen" from "seen with empty value"
/// from "seen with a non-empty value", so that duplicate
/// detection works correctly and error messages are precise.
/// Each set variant records the 1-based line number where the
/// key appeared.
enum CfgValue {
    /// The key has not appeared in the input.
    NotSeen,
    /// The key appeared with an empty value (e.g.
    /// `target_abi=""`).
    Empty {
        /// The 1-based line number.
        line: usize,
    },
    /// The key appeared with a non-empty value.
    Value {
        value: String,
        /// The 1-based line number.
        line: usize,
    },
}

impl CfgValue {
    /// Sets the value from a parsed string. Returns an error
    /// if the key has already been seen. Empty strings become
    /// `Empty`; non-empty strings become `Value`.
    fn set(
        &mut self,
        value: &str,
        line: usize,
        make_dup_err: impl FnOnce() -> CustomTripleCreateError,
    ) -> Result<(), CustomTripleCreateError> {
        if !matches!(self, CfgValue::NotSeen) {
            return Err(make_dup_err());
        }
        *self = if value.is_empty() {
            CfgValue::Empty { line }
        } else {
            CfgValue::Value {
                value: value.to_owned(),
                line,
            }
        };
        Ok(())
    }

    /// Converts to `Option<String>`, mapping both `NotSeen`
    /// and `Empty` to `None`. Used for optional `TargetInfo`
    /// fields where `None` means "not specified."
    fn into_option(self) -> Option<String> {
        match self {
            CfgValue::NotSeen | CfgValue::Empty { .. } => None,
            CfgValue::Value { value, .. } => Some(value),
        }
    }

    /// Extracts the value and its line number for a required
    /// key. Returns an error if the key was not seen or was
    /// empty; missing-key errors point to `missing_line`
    /// (typically the last line of input).
    fn require(
        self,
        key: &str,
        missing_line: usize,
        make_err: &impl Fn(String, usize) -> CustomTripleCreateError,
    ) -> Result<(String, usize), CustomTripleCreateError> {
        match self {
            CfgValue::NotSeen => Err(make_err(
                format!("missing required key `{key}`"),
                missing_line,
            )),
            CfgValue::Empty { line } => Err(make_err(
                format!("empty value for required key `{key}`"),
                line,
            )),
            CfgValue::Value { value, line } => Ok((value, line)),
        }
    }
}

/// Intermediate representation of parsed `--print=cfg` output.
struct ParsedCfg {
    arch: CfgValue,
    pointer_width: Option<(u8, usize)>,
    os: CfgValue,
    abi: CfgValue,
    env: CfgValue,
    vendor: CfgValue,
    families: Vec<String>,
    endian: CfgValue,
    /// Each entry is `(value, 1-based line number)`.
    has_atomics: Vec<(String, usize)>,
    panic: CfgValue,
    target_features: Vec<String>,
}

impl ParsedCfg {
    fn parse(triple: &str, input: &str) -> Result<Self, CustomTripleCreateError> {
        let mut parsed = ParsedCfg {
            arch: CfgValue::NotSeen,
            pointer_width: None,
            os: CfgValue::NotSeen,
            abi: CfgValue::NotSeen,
            env: CfgValue::NotSeen,
            vendor: CfgValue::NotSeen,
            families: Vec::new(),
            endian: CfgValue::NotSeen,
            has_atomics: Vec::new(),
            panic: CfgValue::NotSeen,
            target_features: Vec::new(),
        };

        for (line_idx, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Try to parse as key="value".
            let Some((key, rest)) = line.split_once('=') else {
                // Bare flag (e.g. `unix`, `debug_assertions`).
                // These are redundant with target_family and other
                // key-value lines; skip them.
                continue;
            };

            // The value should be quoted.
            let value = rest
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or_else(|| CustomTripleCreateError::ParseCfg {
                    triple: triple.to_string(),
                    input: input.to_string(),
                    message: format!("expected quoted value for key `{key}`"),
                    line: line_idx + 1,
                })?;

            // Helper to reject duplicate single-valued keys.
            // Only allocates on error paths.
            let make_dup_err = |key: &str| -> CustomTripleCreateError {
                CustomTripleCreateError::ParseCfg {
                    triple: triple.to_string(),
                    input: input.to_string(),
                    message: format!("duplicate key `{key}`"),
                    line: line_idx + 1,
                }
            };

            let line_number = line_idx + 1;

            match key {
                "target_arch" => {
                    parsed.arch.set(value, line_number, || make_dup_err(key))?;
                }
                "target_pointer_width" => {
                    if parsed.pointer_width.is_some() {
                        return Err(make_dup_err(key));
                    }
                    let width =
                        value
                            .parse::<u8>()
                            .map_err(|err| CustomTripleCreateError::ParseCfg {
                                triple: triple.to_string(),
                                input: input.to_string(),
                                message: format!(
                                    "invalid target_pointer_width \
                                     `{value}`: {err}"
                                ),
                                line: line_number,
                            })?;
                    parsed.pointer_width = Some((width, line_number));
                }
                "target_os" => {
                    parsed.os.set(value, line_number, || make_dup_err(key))?;
                }
                "target_abi" => {
                    parsed.abi.set(value, line_number, || make_dup_err(key))?;
                }
                "target_env" => {
                    parsed.env.set(value, line_number, || make_dup_err(key))?;
                }
                "target_vendor" => {
                    parsed
                        .vendor
                        .set(value, line_number, || make_dup_err(key))?;
                }
                "target_family" => {
                    if !value.is_empty() {
                        parsed.families.push(value.to_owned());
                    }
                }
                "target_endian" => {
                    parsed
                        .endian
                        .set(value, line_number, || make_dup_err(key))?;
                }
                "target_has_atomic" => {
                    parsed.has_atomics.push((value.to_owned(), line_number));
                }
                "panic" => {
                    parsed.panic.set(value, line_number, || make_dup_err(key))?;
                }
                "target_feature" => {
                    if !value.is_empty() {
                        parsed.target_features.push(value.to_owned());
                    }
                }
                // Unrecognized keys are ignored.
                _ => {}
            }
        }

        Ok(parsed)
    }

    fn into_target_info_and_features(
        self,
        triple: Cow<'static, str>,
        input: &str,
    ) -> Result<(TargetInfo, BTreeSet<String>), CustomTripleCreateError> {
        // Count lines so that "missing key" errors can point to
        // the end of the input.
        let line_count = input.lines().count();
        // Use max(1, ..) so that even empty input gets line 1.
        let last_line = line_count.max(1);

        // Closure to construct errors without repeating the
        // triple/input cloning at each call site. Only allocates
        // on error paths.
        let make_err = |message: String, line: usize| CustomTripleCreateError::ParseCfg {
            triple: triple.to_string(),
            input: input.to_string(),
            message,
            line,
        };

        let (arch, _arch_line) = self.arch.require("target_arch", last_line, &make_err)?;

        let (pointer_width, _pw_line) = self.pointer_width.ok_or_else(|| {
            make_err(
                "missing required key `target_pointer_width`".to_string(),
                last_line,
            )
        })?;

        let (endian_str, endian_line) =
            self.endian.require("target_endian", last_line, &make_err)?;
        let endian = match endian_str.as_str() {
            "little" => Endian::little,
            "big" => Endian::big,
            other => {
                return Err(make_err(
                    format!("unknown target_endian value `{other}`"),
                    endian_line,
                ));
            }
        };

        let mut has_atomics = Vec::with_capacity(self.has_atomics.len());
        for (value, value_line) in &self.has_atomics {
            let ha: HasAtomic = value.parse().map_err(|err| {
                make_err(
                    format!(
                        "invalid target_has_atomic value \
                         `{value}`: {err}"
                    ),
                    *value_line,
                )
            })?;
            has_atomics.push(ha);
        }

        let (panic_str, _panic_line) = self.panic.require("panic", last_line, &make_err)?;
        let panic = Panic::new(panic_str);

        let target_info = TargetInfo {
            triple: Triple::new(triple),
            os: self.os.into_option().map(Os::new),
            abi: self.abi.into_option().map(Abi::new),
            arch: Arch::new(arch),
            env: self.env.into_option().map(Env::new),
            vendor: self.vendor.into_option().map(Vendor::new),
            families: Families::new(self.families.into_iter().map(Family::new)),
            pointer_width,
            endian,
            has_atomics: HasAtomics::new(has_atomics),
            panic,
        };

        let target_features: BTreeSet<String> = self.target_features.into_iter().collect();

        Ok((target_info, target_features))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfg_expr::targets::get_builtin_target_by_triple;
    use std::process::Command;

    #[test]
    fn test_parse_x86_64_linux() {
        let cfg_text = indoc::indoc! {r#"
            debug_assertions
            panic="unwind"
            target_abi=""
            target_arch="x86_64"
            target_endian="little"
            target_env="gnu"
            target_family="unix"
            target_feature="fxsr"
            target_feature="sse"
            target_feature="sse2"
            target_has_atomic="16"
            target_has_atomic="32"
            target_has_atomic="64"
            target_has_atomic="8"
            target_has_atomic="ptr"
            target_os="linux"
            target_pointer_width="64"
            target_vendor="unknown"
            unix
        "#};

        let (info, features) = parse_cfg_output("x86_64-unknown-linux-gnu".into(), cfg_text)
            .expect("parsed successfully");

        assert_eq!(info.arch.as_str(), "x86_64");
        assert_eq!(info.pointer_width, 64);
        assert_eq!(info.os.as_ref().map(|o| o.as_str()), Some("linux"));
        // Empty abi should be None.
        assert!(info.abi.is_none());
        assert_eq!(info.env.as_ref().map(|e| e.as_str()), Some("gnu"));
        assert_eq!(info.vendor.as_ref().map(|v| v.as_str()), Some("unknown"));
        assert!(info.families.contains(&Family::unix));
        assert_eq!(info.endian, Endian::little);
        assert!(info.has_atomics.contains(HasAtomic::IntegerSize(8)));
        assert!(info.has_atomics.contains(HasAtomic::IntegerSize(64)));
        assert!(info.has_atomics.contains(HasAtomic::Pointer));
        assert_eq!(info.panic, Panic::unwind);

        assert!(features.contains("fxsr"));
        assert!(features.contains("sse"));
        assert!(features.contains("sse2"));
        assert_eq!(features.len(), 3);
    }

    #[test]
    fn test_parse_big_endian() {
        let cfg_text = indoc::indoc! {r#"
            panic="unwind"
            target_arch="powerpc"
            target_endian="big"
            target_env="gnu"
            target_family="unix"
            target_has_atomic="16"
            target_has_atomic="32"
            target_has_atomic="64"
            target_has_atomic="8"
            target_has_atomic="ptr"
            target_os="linux"
            target_pointer_width="32"
            target_vendor="unknown"
        "#};

        let (info, _features) = parse_cfg_output("powerpc-unknown-linux-gnu".into(), cfg_text)
            .expect("parsed successfully");

        assert_eq!(info.endian, Endian::big);
        assert_eq!(info.pointer_width, 32);
        assert_eq!(info.arch.as_str(), "powerpc");
    }

    #[test]
    fn test_parse_multiple_families() {
        let cfg_text = indoc::indoc! {r#"
            panic="abort"
            target_arch="wasm32"
            target_endian="little"
            target_family="wasm"
            target_os="unknown"
            target_pointer_width="32"
        "#};

        let (info, _features) = parse_cfg_output("wasm32-unknown-unknown".into(), cfg_text)
            .expect("parsed successfully");

        assert!(info.families.contains(&Family::wasm));
    }

    #[test]
    fn test_missing_required_arch() {
        let cfg_text = indoc::indoc! {r#"
            target_pointer_width="64"
            target_os="linux"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail without target_arch");

        let message = err.to_string();
        assert!(
            message.contains("some-triple"),
            "error mentions triple: {message}"
        );
    }

    #[test]
    fn test_missing_required_pointer_width() {
        let cfg_text = indoc::indoc! {r#"
            target_arch="x86_64"
            target_os="linux"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail without target_pointer_width");

        let message = err.to_string();
        assert!(
            message.contains("some-triple"),
            "error mentions triple: {message}"
        );
    }

    #[test]
    fn test_malformed_quoting() {
        let cfg_text = "target_arch=x86_64\n";

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail with unquoted value");

        match &err {
            CustomTripleCreateError::ParseCfg { line, message, .. } => {
                assert_eq!(*line, 1);
                assert!(
                    message.contains("quoted"),
                    "error mentions quoting: {message}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_invalid_pointer_width() {
        let cfg_text = indoc::indoc! {r#"
            target_arch="x86_64"
            target_pointer_width="not_a_number"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail with invalid pointer width");

        match &err {
            CustomTripleCreateError::ParseCfg { message, .. } => {
                assert!(
                    message.contains("target_pointer_width"),
                    "error mentions field: {message}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_missing_required_endian() {
        let cfg_text = indoc::indoc! {r#"
            panic="unwind"
            target_arch="x86_64"
            target_os="linux"
            target_pointer_width="64"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail without target_endian");

        match &err {
            CustomTripleCreateError::ParseCfg { message, .. } => {
                assert!(
                    message.contains("target_endian"),
                    "error mentions field: {message}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_missing_required_panic() {
        let cfg_text = indoc::indoc! {r#"
            target_arch="x86_64"
            target_endian="little"
            target_os="linux"
            target_pointer_width="64"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail without panic");

        match &err {
            CustomTripleCreateError::ParseCfg { message, .. } => {
                assert!(message.contains("panic"), "error mentions field: {message}");
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_invalid_has_atomic() {
        let cfg_text = indoc::indoc! {r#"
            panic="unwind"
            target_arch="x86_64"
            target_endian="little"
            target_os="linux"
            target_pointer_width="64"
            target_has_atomic="banana"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail with invalid target_has_atomic");

        match &err {
            CustomTripleCreateError::ParseCfg { message, line, .. } => {
                assert!(
                    message.contains("target_has_atomic"),
                    "error mentions field: {message}"
                );
                assert!(
                    message.contains("banana"),
                    "error mentions value: {message}"
                );
                // The invalid value is on line 6.
                assert_eq!(*line, 6);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_unknown_endian_value() {
        let cfg_text = indoc::indoc! {r#"
            panic="unwind"
            target_arch="x86_64"
            target_endian="middle"
            target_os="linux"
            target_pointer_width="64"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail with unknown endian");

        match &err {
            CustomTripleCreateError::ParseCfg { message, line, .. } => {
                assert!(
                    message.contains("target_endian"),
                    "error mentions field: {message}"
                );
                assert!(
                    message.contains("middle"),
                    "error mentions value: {message}"
                );
                // The bad value is on line 3.
                assert_eq!(*line, 3);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_parse_bare_metal_target() {
        // Simulates a bare-metal target like
        // thumbv7em-none-eabihf: no OS, empty vendor/env/abi.
        let cfg_text = indoc::indoc! {r#"
            panic="abort"
            target_abi=""
            target_arch="arm"
            target_endian="little"
            target_env=""
            target_os="none"
            target_pointer_width="32"
            target_vendor=""
        "#};

        let (info, features) = parse_cfg_output("thumbv7em-none-eabihf".into(), cfg_text)
            .expect("parsed successfully");

        assert_eq!(info.arch.as_str(), "arm");
        assert_eq!(info.pointer_width, 32);
        // "none" is a real OS value, not empty.
        assert_eq!(info.os.as_ref().map(|o| o.as_str()), Some("none"));
        // Empty strings should map to None.
        assert!(info.abi.is_none());
        assert!(info.env.is_none());
        assert!(info.vendor.is_none());
        assert!(info.families.is_empty());
        assert!(info.has_atomics.is_empty());
        assert_eq!(info.panic, Panic::abort);
        assert!(features.is_empty());
    }

    #[test]
    fn test_duplicate_single_valued_key() {
        let cfg_text = indoc::indoc! {r#"
            panic="unwind"
            target_arch="x86_64"
            target_arch="aarch64"
            target_endian="little"
            target_os="linux"
            target_pointer_width="64"
        "#};

        let err = parse_cfg_output("some-triple".into(), cfg_text)
            .expect_err("should fail with duplicate key");

        match &err {
            CustomTripleCreateError::ParseCfg { message, line, .. } => {
                assert!(
                    message.contains("duplicate"),
                    "error mentions duplicate: {message}"
                );
                assert!(
                    message.contains("target_arch"),
                    "error mentions key: {message}"
                );
                // The duplicate is on line 3.
                assert_eq!(*line, 3);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    /// Integration test: run `rustc --print=cfg` for the host
    /// target and verify the parsed result matches the builtin
    /// target info.
    #[test]
    fn test_roundtrip_against_rustc() {
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());

        // Get the host triple.
        let vv_output = Command::new(&rustc)
            .arg("-vV")
            .output()
            .expect("rustc -vV succeeded");
        assert!(vv_output.status.success());
        let vv_text = String::from_utf8(vv_output.stdout).unwrap();
        let host_triple = vv_text
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .expect("found host triple");

        // Get cfg output for the host.
        let cfg_output = Command::new(&rustc)
            .args(["--print", "cfg"])
            .output()
            .expect("rustc --print cfg succeeded");
        assert!(cfg_output.status.success());
        let cfg_text = String::from_utf8(cfg_output.stdout).unwrap();

        // Parse it.
        let (info, _features) = parse_cfg_output(host_triple.to_owned().into(), &cfg_text)
            .expect("parsed host cfg output");

        // Compare against the builtin target info.
        let builtin = get_builtin_target_by_triple(host_triple).expect("host triple is builtin");

        assert_eq!(info.arch.as_str(), builtin.arch.as_str(), "arch matches");
        assert_eq!(
            info.pointer_width, builtin.pointer_width,
            "pointer_width matches"
        );
        assert_eq!(
            info.os.as_ref().map(|o| o.as_str()),
            builtin.os.as_ref().map(|o| o.as_str()),
            "os matches"
        );
        assert_eq!(
            info.env.as_ref().map(|e| e.as_str()),
            builtin.env.as_ref().map(|e| e.as_str()),
            "env matches"
        );
        assert_eq!(info.endian, builtin.endian, "endian matches");
    }
}
