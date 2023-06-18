// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Parse custom platforms.

use std::borrow::Cow;

use cfg_expr::targets::{
    Abi, Arch, Env, Families, Family, HasAtomic, HasAtomics, Os, TargetInfo, Triple, Vendor,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct TargetDefinition {
    arch: String,
    #[serde(rename = "target-pointer-width", with = "target_pointer_width")]
    pointer_width: u8,

    // These parameters are not used by target-spec but are mandatory in Target, so we require them
    // here. https://doc.rust-lang.org/nightly/nightly-rustc/rustc_target/spec/struct.Target.html
    #[allow(dead_code)]
    llvm_target: String,
    #[allow(dead_code)]
    data_layout: String,

    // These are optional parameters used by target-spec.
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    abi: Option<String>,
    #[serde(default)]
    env: Option<String>,
    #[serde(default)]
    vendor: Option<String>,
    #[serde(default)]
    families: Vec<String>,
    #[serde(default)]
    endian: Endian,
    #[serde(default)]
    min_atomic_width: Option<u16>,
    #[serde(default)]
    max_atomic_width: Option<u16>,
    #[serde(default)]
    panic_strategy: PanicStrategy,
}

impl TargetDefinition {
    pub(crate) fn into_target_info(self, triple: Cow<'static, str>) -> TargetInfo {
        // Per https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_target/spec/mod.rs.html,
        // the default value for min_atomic_width is 8.
        let min_atomic_width = self.min_atomic_width.unwrap_or(8);
        // The default max atomic width is the pointer width.
        let max_atomic_width = self.max_atomic_width.unwrap_or(self.pointer_width as u16);

        let mut has_atomics = Vec::new();
        // atomic_width should always be a power of two, but rather than checking that we just
        // start counting up from 8.
        let mut atomic_width = 8;
        while atomic_width <= max_atomic_width {
            if atomic_width < min_atomic_width {
                atomic_width *= 2;
                continue;
            }
            has_atomics.push(HasAtomic::IntegerSize(atomic_width));
            if atomic_width == self.pointer_width as u16 {
                has_atomics.push(HasAtomic::Pointer);
            }
            atomic_width *= 2;
        }

        TargetInfo {
            triple: Triple::new(triple),
            os: self.os.map(Os::new),
            abi: self.abi.map(Abi::new),
            arch: Arch::new(self.arch),
            env: self.env.map(Env::new),
            vendor: self.vendor.map(Vendor::new),
            families: Families::new(self.families.into_iter().map(Family::new)),
            pointer_width: self.pointer_width,
            endian: self.endian.to_cfg_expr(),
            has_atomics: HasAtomics::new(has_atomics),
            panic: self.panic_strategy.to_cfg_expr(),
        }
    }
}

mod target_pointer_width {
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<u8, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Pointer width is specified as a string.
        let string = String::deserialize(deserializer)?;
        string
            .parse::<u8>()
            .map_err(|error| D::Error::custom(format!("error parsing as integer: {error}")))
    }

    pub(super) fn serialize<S>(value: &u8, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }
}

#[derive(
    Copy, Clone, Debug, Deserialize, Serialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd,
)]
#[serde(rename_all = "kebab-case")]
enum Endian {
    #[default]
    Little,
    Big,
}

impl Endian {
    fn to_cfg_expr(self) -> cfg_expr::targets::Endian {
        match self {
            Self::Little => cfg_expr::targets::Endian::little,
            Self::Big => cfg_expr::targets::Endian::big,
        }
    }
}

#[derive(
    Copy, Clone, Debug, Deserialize, Serialize, Default, Eq, Hash, Ord, PartialEq, PartialOrd,
)]
#[serde(rename_all = "kebab-case")]
enum PanicStrategy {
    #[default]
    Unwind,
    Abort,
}

impl PanicStrategy {
    fn to_cfg_expr(self) -> cfg_expr::targets::Panic {
        match self {
            Self::Unwind => cfg_expr::targets::Panic::unwind,
            Self::Abort => cfg_expr::targets::Panic::abort,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, process::Command};

    #[derive(Deserialize)]
    #[serde(transparent)]
    struct AllTargets(BTreeMap<String, TargetDefinition>);

    #[test]
    fn test_all_builtin_specs_recognized() {
        let version = rustc_version::version().expect("rustc_version succeeded");
        if version.minor < 70 {
            // all-target-specs-json is only present on Rust 1.70 and above.
            println!("** skipping, minor version {} < 70", version.minor);
            return;
        }

        let rustc_bin: String = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
        let output = Command::new(rustc_bin)
            // Used for -Zunstable-options. This is test-only code so it doesn't matter.
            .env("RUSTC_BOOTSTRAP", "1")
            .args(["-Z", "unstable-options", "--print", "all-target-specs-json"])
            .output()
            .expect("rustc command succeeded");
        assert!(output.status.success(), "rustc command succeeded");

        let all_targets: AllTargets = serde_json::from_slice(&output.stdout)
            .expect("deserializing all-target-specs-json succeeded");
        for (triple, target_def) in all_targets.0 {
            eprintln!("*** testing {triple}");
            // Just make sure this doesn't panic. (If this becomes fallible in the future, then this
            // shouldn't return an error either.)
            target_def.clone().into_target_info(triple.into());
            let json =
                serde_json::to_string(&target_def).expect("target def serialized successfully");
            eprintln!("* minified json: {json}");
            let target_def_2 = serde_json::from_str(&json).expect("target def 2 deserialized");
            assert_eq!(target_def, target_def_2, "matches");
        }
    }
}
