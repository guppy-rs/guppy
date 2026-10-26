// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rendering a platform as `rustc --print=cfg` output.
//!
//! [`Platform::cfg_text`] produces the text `rustc` would print for a build
//! targeting the platform. This is the inverse of `Platform::new_custom_cfg`.

use crate::{Platform, TargetFeatures, Triple};
use cfg_expr::{
    TargetPredicate,
    targets::{
        ALL_BUILTINS, Abi, Arch, Endian, Env, Family, HasAtomic, Os, Panic, TargetInfo, Vendor,
    },
};
use std::{collections::BTreeSet, sync::OnceLock};

impl Platform {
    /// Returns the configuration options `rustc --print=cfg` would print
    /// for a build targeting this platform, in that format: one option per
    /// line, sorted, as `name` or `key="value"`.
    ///
    /// The options describe the target: `target_arch`, `target_os`,
    /// `target_env`, `target_abi`, `target_vendor`, `target_family` (with
    /// the `unix` and `windows` shorthands), `target_endian`,
    /// `target_pointer_width`, `target_has_atomic`, `panic`, and
    /// `target_feature` for each feature if the platform's
    /// [`TargetFeatures`] are known. Flags added with
    /// [`add_flags`](Self::add_flags) are included as bare names. Build
    /// profile options, e.g. `debug_assertions`, are not included since the
    /// platform does not know them.
    ///
    /// For [builtin](Self::is_builtin) and [custom](Self::is_custom)
    /// platforms the text comes straight from the target info, and parsing
    /// it with [`new_custom_cfg`](Self::new_custom_cfg) produces a platform
    /// that renders the same text. [Heuristic](Self::is_heuristic) platforms
    /// are probed with every value found in the builtin targets, so values
    /// outside that set are not reported. Note that in both cases the text
    /// agrees with [`TargetSpec::eval`](crate::TargetSpec::eval).
    ///
    /// # Examples
    ///
    /// ```
    /// use target_spec::{Platform, TargetFeatures};
    ///
    /// let platform = Platform::new("x86_64-unknown-linux-gnu", TargetFeatures::Unknown).unwrap();
    /// let text = platform.cfg_text();
    /// assert!(text.lines().any(|line| line == r#"target_os="linux""#));
    /// assert!(text.lines().any(|line| line == "unix"));
    /// ```
    pub fn cfg_text(&self) -> String {
        let mut lines = match self.triple().target_info() {
            Some(target_info) => target_info_lines(target_info),
            None => probed_lines(self.triple()),
        };
        if let TargetFeatures::Features(features) = self.target_features() {
            lines.extend(
                features
                    .iter()
                    .map(|feature| pair("target_feature", feature)),
            );
        }
        lines.extend(self.flags().map(str::to_owned));
        lines.sort();
        lines.dedup();
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// A `key="value"` line.
fn pair(key: &str, value: &str) -> String {
    format!("{key}=\"{value}\"")
}

/// The lines for a family: the key-value pair, and for `unix` and `windows`
/// the bare shorthand `rustc` also prints.
fn family_lines(family: &str) -> impl Iterator<Item = String> {
    let shorthand = match family {
        "unix" | "windows" => Some(family.to_owned()),
        _ => None,
    };
    std::iter::once(pair("target_family", family)).chain(shorthand)
}

/// The lines of a target whose information is known exactly.
///
/// `None` fields are rendered the way `rustc` prints them and the way
/// [`TargetPredicate`] matching interprets them: an empty `target_abi` and
/// `target_env`, `target_os="none"`, and `target_vendor="unknown"`.
fn target_info_lines(info: &TargetInfo) -> Vec<String> {
    let mut lines = vec![
        pair("panic", info.panic.as_str()),
        pair("target_abi", info.abi.as_ref().map_or("", Abi::as_str)),
        pair("target_arch", info.arch.as_str()),
        pair("target_endian", endian_str(info.endian)),
        pair("target_env", info.env.as_ref().map_or("", Env::as_str)),
        pair("target_os", info.os.as_ref().map_or("none", Os::as_str)),
        pair("target_pointer_width", &info.pointer_width.to_string()),
        pair(
            "target_vendor",
            info.vendor.as_ref().map_or("unknown", Vendor::as_str),
        ),
    ];
    lines.extend(
        info.families
            .iter()
            .flat_map(|family| family_lines(family.as_str())),
    );
    lines.extend(
        info.has_atomics
            .iter()
            .map(|has_atomic| pair("target_has_atomic", &has_atomic.to_string())),
    );
    lines
}

fn endian_str(endian: Endian) -> &'static str {
    match endian {
        Endian::big => "big",
        Endian::little => "little",
    }
}

/// The distinct values of each key across the builtin targets, plus the
/// values matching treats specially. These are the candidates probed for
/// heuristic platforms.
struct Candidates {
    abis: BTreeSet<&'static str>,
    arches: BTreeSet<&'static str>,
    envs: BTreeSet<&'static str>,
    oses: BTreeSet<&'static str>,
    vendors: BTreeSet<&'static str>,
    families: BTreeSet<&'static str>,
    pointer_widths: BTreeSet<u8>,
    has_atomics: BTreeSet<HasAtomic>,
    panics: BTreeSet<&'static str>,
}

fn candidates() -> &'static Candidates {
    static CANDIDATES: OnceLock<Candidates> = OnceLock::new();
    CANDIDATES.get_or_init(|| {
        let mut c = Candidates {
            abis: BTreeSet::from([""]),
            arches: BTreeSet::new(),
            envs: BTreeSet::from([""]),
            oses: BTreeSet::from(["none"]),
            vendors: BTreeSet::from(["unknown"]),
            families: BTreeSet::new(),
            pointer_widths: BTreeSet::new(),
            has_atomics: BTreeSet::new(),
            panics: BTreeSet::new(),
        };
        for info in ALL_BUILTINS {
            c.abis.extend(info.abi.as_ref().map(Abi::as_str));
            c.arches.insert(info.arch.as_str());
            c.envs.extend(info.env.as_ref().map(Env::as_str));
            c.oses.extend(info.os.as_ref().map(Os::as_str));
            c.vendors.extend(info.vendor.as_ref().map(Vendor::as_str));
            c.families.extend(info.families.iter().map(Family::as_str));
            c.pointer_widths.insert(info.pointer_width);
            c.has_atomics.extend(info.has_atomics.iter().copied());
            c.panics.insert(info.panic.as_str());
        }
        c
    })
}

/// The lines of a heuristic platform, found by evaluating each candidate
/// value against the triple.
fn probed_lines(triple: &Triple) -> Vec<String> {
    let c = candidates();
    let mut lines = Vec::new();
    let mut probe = |key: &str, value: &str, tp: TargetPredicate| {
        if triple.matches(&tp) {
            lines.push(pair(key, value));
        }
    };

    for &v in &c.panics {
        probe("panic", v, TargetPredicate::Panic(Panic::new(v)));
    }
    for &v in &c.abis {
        probe("target_abi", v, TargetPredicate::Abi(Abi::new(v)));
    }
    for &v in &c.arches {
        probe("target_arch", v, TargetPredicate::Arch(Arch::new(v)));
    }
    for (endian, v) in [(Endian::big, "big"), (Endian::little, "little")] {
        probe("target_endian", v, TargetPredicate::Endian(endian));
    }
    for &v in &c.envs {
        probe("target_env", v, TargetPredicate::Env(Env::new(v)));
    }
    for &v in &c.oses {
        probe("target_os", v, TargetPredicate::Os(Os::new(v)));
    }
    for &v in &c.vendors {
        probe("target_vendor", v, TargetPredicate::Vendor(Vendor::new(v)));
    }
    for &width in &c.pointer_widths {
        if triple.matches(&TargetPredicate::PointerWidth(width)) {
            lines.push(pair("target_pointer_width", &width.to_string()));
        }
    }
    for &v in &c.families {
        if triple.matches(&TargetPredicate::Family(Family::new(v))) {
            lines.extend(family_lines(v));
        }
    }
    for &has_atomic in &c.has_atomics {
        if triple.matches(&TargetPredicate::HasAtomic(has_atomic)) {
            lines.push(pair("target_has_atomic", &has_atomic.to_string()));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TargetSpec;

    fn lines(platform: &Platform) -> Vec<String> {
        platform.cfg_text().lines().map(str::to_owned).collect()
    }

    /// Splits a `key="value"` line; bare names have no value.
    fn split(line: &str) -> (&str, Option<&str>) {
        match line.split_once('=') {
            Some((key, value)) => (key, Some(value.trim_matches('"'))),
            None => (line, None),
        }
    }

    #[test]
    fn x86_64_linux_matches_rustc_output() {
        let platform = Platform::new("x86_64-unknown-linux-gnu", TargetFeatures::Unknown).unwrap();
        // What `rustc --print=cfg --target x86_64-unknown-linux-gnu`
        // prints, minus the build profile lines (`debug_assertions`,
        // `overflow_checks`, ...) and the `target_feature` lines, which
        // the platform does not know.
        let expected = indoc::indoc! {r#"
            panic="unwind"
            target_abi=""
            target_arch="x86_64"
            target_endian="little"
            target_env="gnu"
            target_family="unix"
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
        assert_eq!(platform.cfg_text(), expected);
    }

    #[test]
    fn features_and_flags_are_included() {
        let mut platform = Platform::new(
            "x86_64-pc-windows-msvc",
            TargetFeatures::features(["sse2", "avx"]),
        )
        .unwrap();
        platform.add_flags(["cargo_web"]);
        let lines = lines(&platform);
        assert!(lines.contains(&"cargo_web".to_owned()));
        assert!(lines.contains(&r#"target_feature="avx""#.to_owned()));
        assert!(lines.contains(&r#"target_feature="sse2""#.to_owned()));
        assert!(lines.contains(&"windows".to_owned()));
        assert!(lines.contains(&r#"target_family="windows""#.to_owned()));
        assert!(!lines.contains(&"unix".to_owned()));
    }

    #[test]
    fn unknown_features_are_omitted() {
        for features in [TargetFeatures::Unknown, TargetFeatures::All] {
            let platform = Platform::new("x86_64-unknown-linux-gnu", features).unwrap();
            assert!(
                lines(&platform)
                    .iter()
                    .all(|line| split(line).0 != "target_feature")
            );
        }
    }

    #[test]
    fn bare_metal_target() {
        let platform = Platform::new("thumbv7em-none-eabihf", TargetFeatures::Unknown).unwrap();
        let lines = lines(&platform);
        assert!(lines.contains(&r#"target_os="none""#.to_owned()));
        assert!(lines.contains(&r#"target_env="""#.to_owned()));
        assert!(lines.contains(&r#"panic="abort""#.to_owned()));
        assert!(!lines.iter().any(|line| line.starts_with("target_family")));
    }

    #[test]
    fn heuristic_platform_is_probed() {
        let platform = Platform::new("armv5te-apple-darwin", TargetFeatures::Unknown).unwrap();
        assert!(platform.is_heuristic());
        let lines = lines(&platform);
        assert!(lines.contains(&r#"target_arch="arm""#.to_owned()));
        assert!(lines.contains(&r#"target_os="macos""#.to_owned()));
        assert!(lines.contains(&r#"target_vendor="apple""#.to_owned()));
        assert!(lines.contains(&r#"target_endian="little""#.to_owned()));
        assert!(lines.contains(&r#"target_pointer_width="32""#.to_owned()));
        assert!(lines.contains(&"unix".to_owned()));
    }

    /// Every option a platform reports evaluates to true against it, and
    /// for the single-valued keys, every other builtin value evaluates to
    /// false. This holds for every builtin platform and for a sample of
    /// heuristic ones.
    #[test]
    fn cfg_text_agrees_with_eval() {
        let heuristic = ["armv5te-apple-darwin", "x86_64-unknown-openbsd-gnu"];
        let platforms = ALL_BUILTINS
            .iter()
            .map(|info| info.triple.as_str())
            .chain(heuristic)
            .map(|triple| Platform::new(triple.to_owned(), TargetFeatures::Unknown).unwrap());
        let c = candidates();
        let single_valued: Vec<(&str, Vec<String>)> = vec![
            ("panic", c.panics.iter().map(|s| s.to_string()).collect()),
            ("target_abi", c.abis.iter().map(|s| s.to_string()).collect()),
            (
                "target_arch",
                c.arches.iter().map(|s| s.to_string()).collect(),
            ),
            ("target_endian", vec!["big".into(), "little".into()]),
            ("target_env", c.envs.iter().map(|s| s.to_string()).collect()),
            ("target_os", c.oses.iter().map(|s| s.to_string()).collect()),
            (
                "target_pointer_width",
                c.pointer_widths.iter().map(|w| w.to_string()).collect(),
            ),
            (
                "target_vendor",
                c.vendors.iter().map(|s| s.to_string()).collect(),
            ),
        ];

        for platform in platforms {
            let lines = lines(&platform);
            for line in &lines {
                let expr = match split(line) {
                    (name, None) => format!("cfg({name})"),
                    (key, Some(value)) => format!("cfg({key} = \"{value}\")"),
                };
                let spec = TargetSpec::new(expr.clone()).unwrap();
                assert_eq!(
                    spec.eval(&platform),
                    Some(true),
                    "{expr} holds for {}",
                    platform.triple_str()
                );
            }
            for (key, values) in &single_valued {
                let reported: Vec<_> = lines
                    .iter()
                    .map(|line| split(line))
                    .filter(|(k, _)| k == key)
                    .filter_map(|(_, v)| v)
                    .collect();
                assert!(
                    reported.len() <= 1,
                    "{key} is single-valued for {}: {reported:?}",
                    platform.triple_str()
                );
                for value in values {
                    if reported.contains(&value.as_str()) {
                        continue;
                    }
                    let expr = format!("cfg({key} = \"{value}\")");
                    let spec = TargetSpec::new(expr.clone()).unwrap();
                    assert_eq!(
                        spec.eval(&platform),
                        Some(false),
                        "{expr} does not hold for {}",
                        platform.triple_str()
                    );
                }
            }
        }
    }

    /// The text of a builtin platform parses back as a custom platform
    /// that renders the same text.
    #[cfg(feature = "custom-cfg")]
    #[test]
    fn roundtrip_through_custom_cfg() {
        for info in ALL_BUILTINS {
            let triple = info.triple.as_str();
            let platform = Platform::new(triple.to_owned(), TargetFeatures::Unknown).unwrap();
            let text = platform.cfg_text();
            let custom =
                Platform::new_custom_cfg(triple.to_owned(), &text, TargetFeatures::Unknown)
                    .unwrap_or_else(|err| panic!("{triple}: {err}"));
            assert_eq!(custom.cfg_text(), text, "{triple} round-trips");
        }
    }
}
