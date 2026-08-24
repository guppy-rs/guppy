// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{errors::TargetSpecError, platform::PlatformSpec};
use std::{error, fmt, sync::Arc};
pub use target_spec::summaries::{PlatformSummary, TargetFeaturesSummary};

/// A serializable version of [`PlatformSpec`].
///
/// Requires the `summaries` feature to be enabled.
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub enum PlatformSpecSummary {
    /// The intersection of all platforms.
    ///
    /// This is converted to and from [`PlatformSpec::Always`], and is expressed as the string
    /// `"always"`, or as `spec = "always"`.
    ///
    /// # Examples
    ///
    /// Deserialize the string `"always"`.
    ///
    /// ```
    /// # use guppy::platform::PlatformSpecSummary;
    /// let spec: PlatformSpecSummary = serde_json::from_str(r#""always""#).unwrap();
    /// assert_eq!(spec, PlatformSpecSummary::Always);
    /// ```
    ///
    /// Deserialize `spec = "always"`.
    ///
    /// ```
    /// # use guppy::platform::PlatformSpecSummary;
    /// let spec: PlatformSpecSummary = toml::from_str(r#"spec = "always""#).unwrap();
    /// assert_eq!(spec, PlatformSpecSummary::Always);
    /// ```
    Always,

    /// A list of platforms.
    ///
    /// This is converted to and from [`PlatformSpec::Platforms`].
    ///
    /// * If the list has one platform, it is serialized as that platform's
    ///   table (for example `{ triple = "x86_64-unknown-linux-gnu",
    ///   target-features = "unknown" }`) rather than as a one-element array,
    ///   so files written by earlier versions of `guppy` round-trip unchanged.
    /// * Otherwise (zero, or two or more platforms), it is serialized as an
    ///   array of platform summaries.
    ///
    /// The deserializer accepts the following forms that the serializer does not produce:
    ///
    /// * A bare triple string, which is shorthand for a table with only `triple` set.
    /// * A one-element array, which is treated as a single platform.
    ///
    /// An empty array is the union of zero platforms, on which nothing is
    /// enabled. Note that this is different from omitting the key entirely,
    /// which is turned into `Any`.
    ///
    /// The strings `"always"` and `"any"` are only recognized as the whole
    /// spec: inside an array they are rejected, since they are not platforms.
    ///
    /// # Examples
    ///
    /// Deserialize a target triple.
    ///
    /// ```
    /// # use guppy::platform::{PlatformSummary, PlatformSpecSummary};
    /// # use target_spec::summaries::TargetFeaturesSummary;
    /// # use std::collections::BTreeSet;
    /// let spec: PlatformSpecSummary = serde_json::from_str(r#""x86_64-unknown-linux-gnu""#).unwrap();
    /// assert_eq!(
    ///     spec,
    ///     PlatformSpecSummary::Platforms(vec![PlatformSummary::new("x86_64-unknown-linux-gnu")]),
    /// );
    /// ```
    ///
    /// Deserialize a target map.
    ///
    /// ```
    /// # use guppy::platform::{PlatformSummary, PlatformSpecSummary};
    /// # use target_spec::summaries::TargetFeaturesSummary;
    /// # use std::collections::BTreeSet;
    /// let spec: PlatformSpecSummary = toml::from_str(r#"
    /// triple = "x86_64-unknown-linux-gnu"
    /// target-features = []
    /// flags = []
    /// "#).unwrap();
    /// assert_eq!(
    ///     spec,
    ///     PlatformSpecSummary::Platforms(vec![
    ///         PlatformSummary::new("x86_64-unknown-linux-gnu")
    ///             .with_target_features(TargetFeaturesSummary::Features(BTreeSet::new()))
    ///     ])
    /// );
    /// ```
    ///
    /// Deserialize a list of platforms.
    ///
    /// ```
    /// # use guppy::platform::{PlatformSummary, PlatformSpecSummary};
    /// let spec: PlatformSpecSummary = serde_json::from_str(r#"
    /// ["x86_64-unknown-linux-gnu", { "triple": "x86_64-pc-windows-msvc" }]
    /// "#).unwrap();
    /// assert_eq!(
    ///     spec,
    ///     PlatformSpecSummary::Platforms(vec![
    ///         PlatformSummary::new("x86_64-unknown-linux-gnu"),
    ///         PlatformSummary::new("x86_64-pc-windows-msvc"),
    ///     ]),
    /// );
    /// ```
    Platforms(Vec<PlatformSummary>),

    /// The union of all platforms.
    ///
    /// This is converted to and from [`PlatformSpec::Any`], and is serialized as the string
    /// `"any"`.
    ///
    /// This is also the default, since in many cases one desires to compute the union of enabled
    /// dependencies across all platforms.
    ///
    /// # Examples
    ///
    /// Deserialize the string `"any"`.
    ///
    /// ```
    /// # use guppy::platform::PlatformSpecSummary;
    /// let spec: PlatformSpecSummary = serde_json::from_str(r#""any""#).unwrap();
    /// assert_eq!(spec, PlatformSpecSummary::Any);
    /// ```
    ///
    /// Deserialize `spec = "any"`.
    ///
    /// ```
    /// # use guppy::platform::PlatformSpecSummary;
    /// let spec: PlatformSpecSummary = toml::from_str(r#"spec = "any""#).unwrap();
    /// assert_eq!(spec, PlatformSpecSummary::Any);
    /// ```
    #[default]
    Any,
}

impl PlatformSpecSummary {
    /// Creates a new `PlatformSpecSummary` from a [`PlatformSpec`].
    pub fn new(platform_spec: &PlatformSpec) -> Self {
        match platform_spec {
            PlatformSpec::Always => PlatformSpecSummary::Always,
            PlatformSpec::Platforms(platforms) => PlatformSpecSummary::Platforms(
                platforms
                    .iter()
                    .map(|platform| platform.to_summary())
                    .collect(),
            ),
            PlatformSpec::Any => PlatformSpecSummary::Any,
        }
    }

    /// Converts `self` to a `PlatformSpec`.
    ///
    /// Returns an error naming the platform if it could not be converted, for
    /// example because its triple was unknown.
    pub fn to_platform_spec(&self) -> Result<PlatformSpec, PlatformSpecSummaryError> {
        match self {
            PlatformSpecSummary::Always => Ok(PlatformSpec::Always),
            PlatformSpecSummary::Platforms(platforms) => {
                let count = platforms.len();
                let platforms = platforms
                    .iter()
                    .enumerate()
                    .map(|(index, platform)| {
                        let platform =
                            platform
                                .to_platform()
                                .map_err(|source| PlatformSpecSummaryError {
                                    triple: platform.triple.clone(),
                                    index,
                                    count,
                                    source: Box::new(source),
                                })?;
                        Ok(Arc::new(platform))
                    })
                    .collect::<Result<Vec<_>, PlatformSpecSummaryError>>()?;
                Ok(PlatformSpec::Platforms(platforms))
            }
            PlatformSpecSummary::Any => Ok(PlatformSpec::Any),
        }
    }

    /// Returns true if `self` is `PlatformSpecSummary::Any`.
    pub fn is_any(&self) -> bool {
        match self {
            PlatformSpecSummary::Any => true,
            PlatformSpecSummary::Always | PlatformSpecSummary::Platforms(_) => false,
        }
    }
}

/// An error returned by [`PlatformSpecSummary::to_platform_spec`].
#[derive(Debug)]
pub struct PlatformSpecSummaryError {
    triple: String,
    index: usize,
    count: usize,
    source: Box<TargetSpecError>,
}

impl PlatformSpecSummaryError {
    /// Returns the triple of the platform that failed.
    pub fn triple(&self) -> &str {
        &self.triple
    }

    /// Returns the underlying target-spec error.
    ///
    /// This error is also returned by `Error::source` for this error.
    pub fn target_spec_error(&self) -> &TargetSpecError {
        &self.source
    }

    /// Returns the zero-based position of the failed platform in the list.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the total number of platforms in the list.
    ///
    /// Returns 1 for a bare platform.
    pub fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn fmt_position(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.count > 1 {
            write!(f, " (element {} of {})", self.index + 1, self.count)?;
        }
        Ok(())
    }
}

impl fmt::Display for PlatformSpecSummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid platform `{}`", self.triple)?;
        self.fmt_position(f)
    }
}

impl error::Error for PlatformSpecSummaryError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&*self.source)
    }
}

mod serde_impl {
    use super::*;
    use serde::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{
            self, DeserializeSeed, IgnoredAny, IntoDeserializer, MapAccess, SeqAccess, Visitor,
            value::{MapAccessDeserializer, StrDeserializer, StringDeserializer},
        },
    };
    use std::fmt;

    impl Serialize for PlatformSpecSummary {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                PlatformSpecSummary::Always => Spec { spec: "always" }.serialize(serializer),
                PlatformSpecSummary::Any => Spec { spec: "any" }.serialize(serializer),
                PlatformSpecSummary::Platforms(platforms) => match platforms.as_slice() {
                    // A single platform is serialized as-is for compatibility
                    // with existing files.
                    [platform] => platform.serialize(serializer),
                    _ => platforms.serialize(serializer),
                },
            }
        }
    }

    // `always` and `any` are serialized as `spec = "always"` tables rather
    // than bare strings. This was originally a workaround for ValueAfterTable
    // errors in older `toml` versions, and is now the on-disk format.
    #[derive(Serialize)]
    struct Spec {
        spec: &'static str,
    }

    impl<'de> Deserialize<'de> for PlatformSpecSummary {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(PlatformSpecSummaryVisitor)
        }
    }

    // This is a hand-written visitor rather than an untagged enum for better
    // error reporting.
    struct PlatformSpecSummaryVisitor;

    impl<'de> Visitor<'de> for PlatformSpecSummaryVisitor {
        type Value = PlatformSpecSummary;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(
                "\"always\", \"any\", a platform (a triple string or a table \
                 with a `triple` key), or an array of platforms",
            )
        }

        fn visit_str<E>(self, spec: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match spec {
                "always" => Ok(PlatformSpecSummary::Always),
                "any" => Ok(PlatformSpecSummary::Any),
                // TODO: expression parsing would go here
                triple => {
                    let platform = platform_from_str(triple)?;
                    Ok(PlatformSpecSummary::Platforms(vec![platform]))
                }
            }
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut platforms = Vec::new();
            while let Some(PlatformElement(platform)) = seq.next_element()? {
                platforms.push(platform);
            }
            Ok(PlatformSpecSummary::Platforms(platforms))
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let first_key = map.next_key::<String>()?;
            if first_key.as_deref() != Some("spec") {
                let platform =
                    platform_from_map(PlatformMap::new(first_key, map, SpecContext::WholeSpec))?;
                return Ok(PlatformSpecSummary::Platforms(vec![platform]));
            }

            let spec: String = map.next_value()?;
            let mut platform_keys = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                map.next_value::<IgnoredAny>()?;
                platform_keys.push(key);
            }
            if !platform_keys.is_empty() {
                return Err(de::Error::custom(format!(
                    "{} cannot appear alongside `spec = \"{spec}\"`: a spec table has \
                     only the `spec` key",
                    format_keys(&platform_keys),
                )));
            }
            match spec.as_str() {
                "always" => Ok(PlatformSpecSummary::Always),
                "any" => Ok(PlatformSpecSummary::Any),
                other => Err(de::Error::custom(format!(
                    "unknown spec `{other}`: expected \"always\" or \"any\" \
                     (a platform is written as `triple = \"...\"`)",
                ))),
            }
        }
    }

    /// One element of a platform array: a platform, but never `"always"`,
    /// `"any"`, or a `spec` table, since those are only meaningful as the
    /// whole spec.
    struct PlatformElement(PlatformSummary);

    impl<'de> Deserialize<'de> for PlatformElement {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(PlatformElementVisitor)
        }
    }

    struct PlatformElementVisitor;

    impl<'de> Visitor<'de> for PlatformElementVisitor {
        type Value = PlatformElement;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a platform (a triple string or a table with a `triple` key)")
        }

        fn visit_str<E>(self, triple: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            platform_from_str(triple).map(PlatformElement)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let first_key = map.next_key::<String>()?;
            platform_from_map(PlatformMap::new(first_key, map, SpecContext::ListElement))
                .map(PlatformElement)
        }
    }

    #[derive(Clone, Copy)]
    enum SpecContext {
        WholeSpec,
        ListElement,
    }

    struct PlatformMap<A> {
        first_key: Option<String>,
        seen_keys: Vec<String>,
        map: A,
        context: SpecContext,
    }

    impl<A> PlatformMap<A> {
        fn new(first_key: Option<String>, map: A, context: SpecContext) -> Self {
            Self {
                first_key,
                seen_keys: Vec::new(),
                map,
                context,
            }
        }
    }

    impl<'de, A: MapAccess<'de>> MapAccess<'de> for PlatformMap<A> {
        type Error = A::Error;

        fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error>
        where
            K: DeserializeSeed<'de>,
        {
            let key = match self.first_key.take() {
                Some(key) => key,
                None => match self.map.next_key::<String>()? {
                    Some(key) => key,
                    None => return Ok(None),
                },
            };
            if key == "spec" {
                return Err(match self.context {
                    SpecContext::WholeSpec => de::Error::custom(format!(
                        "`spec` cannot appear alongside {}: a spec table has only \
                         the `spec` key",
                        format_keys(&self.seen_keys),
                    )),
                    SpecContext::ListElement => de::Error::custom(
                        "`spec` is not allowed inside a list of platforms: \
                         \"always\" and \"any\" are only valid as the whole spec",
                    ),
                });
            }
            self.seen_keys.push(key.clone());
            let deserializer: StringDeserializer<A::Error> = key.into_deserializer();
            seed.deserialize(deserializer).map(Some)
        }

        fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, A::Error>
        where
            V: DeserializeSeed<'de>,
        {
            self.map.next_value_seed(seed)
        }

        fn size_hint(&self) -> Option<usize> {
            self.map
                .size_hint()
                .map(|remaining| remaining + usize::from(self.first_key.is_some()))
        }
    }

    fn format_keys(keys: &[String]) -> String {
        keys.iter()
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn platform_from_str<E: de::Error>(triple: &str) -> Result<PlatformSummary, E> {
        let deserializer: StrDeserializer<'_, E> = triple.into_deserializer();
        let platform = PlatformSummary::deserialize(deserializer)?;
        reject_spec_keyword(&platform.triple)?;
        Ok(platform)
    }

    fn platform_from_map<'de, A: MapAccess<'de>>(
        map: PlatformMap<A>,
    ) -> Result<PlatformSummary, A::Error> {
        let platform = PlatformSummary::deserialize(MapAccessDeserializer::new(map))?;
        reject_spec_keyword(&platform.triple)?;
        Ok(platform)
    }

    fn reject_spec_keyword<E: de::Error>(triple: &str) -> Result<(), E> {
        match triple {
            "always" | "any" => Err(E::custom(format!(
                "`{triple}` is not a platform: \"always\" and \"any\" are only \
                 valid as the whole spec, as a bare string or `spec = \"...\"`",
            ))),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::{collections::BTreeSet, error::Error as _};

    // Wrap in a struct: TOML needs a table at the top level.
    #[derive(Debug, Deserialize, Serialize, Eq, PartialEq)]
    #[serde(rename_all = "kebab-case")]
    pub(super) struct Wrapper {
        #[serde(default)]
        pub(super) target_platform: PlatformSpecSummary,
    }

    const LINUX: &str = "x86_64-unknown-linux-gnu";
    const WINDOWS: &str = "x86_64-pc-windows-msvc";

    fn single_platform(triple: &str) -> PlatformSpecSummary {
        PlatformSpecSummary::Platforms(vec![PlatformSummary::new(triple)])
    }

    fn sse2() -> TargetFeaturesSummary {
        TargetFeaturesSummary::Features(["sse2".to_owned()].into_iter().collect::<BTreeSet<_>>())
    }

    fn mixed_platforms() -> PlatformSpecSummary {
        PlatformSpecSummary::Platforms(vec![
            PlatformSummary::new(LINUX),
            PlatformSummary::new(WINDOWS).with_target_features(sse2()),
        ])
    }

    #[test]
    fn deserialize_platform_lists() {
        let mixed_toml = r#"
        target-platform = [
            "x86_64-unknown-linux-gnu",
            { triple = "x86_64-pc-windows-msvc", target-features = ["sse2"] },
        ]
        "#;
        let wrapper: Wrapper = toml::from_str(mixed_toml).expect("mixed TOML array deserialized");
        assert_eq!(
            wrapper.target_platform,
            mixed_platforms(),
            "string and table elements both deserialize as platforms",
        );

        let mixed_json = r#"{
            "target-platform": [
                "x86_64-unknown-linux-gnu",
                { "triple": "x86_64-pc-windows-msvc", "target-features": ["sse2"] }
            ]
        }"#;
        let wrapper: Wrapper =
            serde_json::from_str(mixed_json).expect("mixed JSON array deserialized");
        assert_eq!(
            wrapper.target_platform,
            mixed_platforms(),
            "the same visitor handles JSON",
        );

        let wrapper: Wrapper =
            toml::from_str("target-platform = []").expect("empty array deserialized");
        assert_eq!(
            wrapper.target_platform,
            PlatformSpecSummary::Platforms(vec![]),
            "an empty array is Platforms over no platforms",
        );

        let wrapper: Wrapper = toml::from_str("").expect("missing key deserialized");
        assert_eq!(
            wrapper.target_platform,
            PlatformSpecSummary::Any,
            "a missing key is Any, not Platforms over no platforms",
        );
    }

    #[test]
    fn deserialize_other_variants_still_win() {
        for (input, expected) in [
            (r#"target-platform = "any""#, PlatformSpecSummary::Any),
            (r#"target-platform = "always""#, PlatformSpecSummary::Always),
            (
                r#"target-platform = { spec = "always" }"#,
                PlatformSpecSummary::Always,
            ),
            (
                r#"target-platform = "x86_64-unknown-linux-gnu""#,
                single_platform(LINUX),
            ),
            (
                r#"target-platform = { triple = "x86_64-unknown-linux-gnu" }"#,
                single_platform(LINUX),
            ),
            (
                r#"target-platform = ["x86_64-unknown-linux-gnu"]"#,
                single_platform(LINUX),
            ),
        ] {
            let wrapper: Wrapper = toml::from_str(input).expect("input deserialized");
            assert_eq!(wrapper.target_platform, expected, "for input {input}");
        }
    }

    #[test]
    fn deserialize_errors_name_the_problem() {
        fn toml_error(input: &str) -> String {
            toml::from_str::<Wrapper>(input)
                .expect_err("input rejected")
                .to_string()
        }

        for (input, expected) in [
            (r#"target-platform = ["any"]"#, "`any` is not a platform"),
            (
                r#"target-platform = ["x86_64-unknown-linux-gnu", "always"]"#,
                "`always` is not a platform",
            ),
            (
                r#"target-platform = [{ triple = "any" }]"#,
                "`any` is not a platform",
            ),
            (
                r#"target-platform = [{ spec = "always" }]"#,
                "`spec` is not allowed inside a list",
            ),
            (
                r#"target-platform = [{ tripel = "x86_64-unknown-linux-gnu" }]"#,
                "unknown field `tripel`",
            ),
            (
                r#"target-platform = [{ target-features = [] }]"#,
                "missing field `triple`",
            ),
            (
                r#"target-platform = [""]"#,
                "a platform triple cannot be empty",
            ),
            (
                r#"target-platform = [
                    "x86_64-unknown-linux-gnu",
                    { triple = "x86_64-pc-windows-msvc", target-features = "bogus" },
                ]"#,
                "unknown string for target features: bogus",
            ),
            (
                r#"target-platform = [["x86_64-unknown-linux-gnu"]]"#,
                "expected a platform (a triple string or a table with a `triple` key)",
            ),
            (
                "target-platform = 5",
                "expected \"always\", \"any\", a platform",
            ),
            (
                r#"target-platform = """#,
                "a platform triple cannot be empty",
            ),
            (
                r#"target-platform = { triple = "" }"#,
                "a platform triple cannot be empty",
            ),
            (
                r#"target-platform = { spec = "always", triple = "x86_64-unknown-linux-gnu" }"#,
                "`triple` cannot appear alongside `spec = \"always\"`",
            ),
            (
                r#"target-platform = { spec = "always", flags = [] }"#,
                "`flags` cannot appear alongside",
            ),
            (
                r#"target-platform = { spec = "always", custom-cfg = "x", target-features = [] }"#,
                "`custom-cfg`, `target-features` cannot appear alongside",
            ),
            (
                r#"target-platform = { triple = "x86_64-unknown-linux-gnu", spec = "always" }"#,
                "`spec` cannot appear alongside `triple`:",
            ),
            (
                r#"target-platform = { triple = "x86_64-unknown-linux-gnu", flags = [], spec = "always" }"#,
                "`spec` cannot appear alongside `triple`, `flags`:",
            ),
            (
                r#"target-platform = [{ triple = "x86_64-unknown-linux-gnu", spec = "always" }]"#,
                "`spec` is not allowed inside a list",
            ),
            (
                r#"target-platform = { spec = "sometimes" }"#,
                "unknown spec `sometimes`",
            ),
            (
                r#"target-platform = { spec = "x86_64-unknown-linux-gnu" }"#,
                "unknown spec `x86_64-unknown-linux-gnu`",
            ),
            (
                r#"target-platform = { target-features = [] }"#,
                "missing field `triple`",
            ),
            (
                r#"target-platform = { triple = "x86_64-unknown-linux-gnu", bogus = 1 }"#,
                "unknown field `bogus`",
            ),
        ] {
            let message = toml_error(input);
            assert!(
                message.contains(expected),
                "for input {input}: error `{message}` contains `{expected}`",
            );
        }

        // The same visitor is used for JSON.
        for (input, expected) in [
            (
                r#"{ "target-platform": ["any"] }"#,
                "`any` is not a platform",
            ),
            (
                r#"{ "target-platform": { "triple": "x86_64-unknown-linux-gnu", "spec": "always" } }"#,
                "`spec` cannot appear alongside `triple`:",
            ),
        ] {
            let message = serde_json::from_str::<Wrapper>(input)
                .expect_err("input rejected")
                .to_string();
            assert!(
                message.contains(expected),
                "for JSON input {input}: error `{message}` contains `{expected}`",
            );
        }
    }

    #[test]
    fn serialize_one_platform_bare_and_others_as_arrays() {
        let render = |target_platform: PlatformSpecSummary| {
            toml::to_string(&Wrapper { target_platform }).expect("summary serialized to TOML")
        };

        assert_eq!(
            render(single_platform(LINUX)),
            "[target-platform]\ntriple = \"x86_64-unknown-linux-gnu\"\n\
             target-features = \"unknown\"\n",
            "one platform serializes bare, not as a one-element array",
        );
        assert_eq!(
            render(PlatformSpecSummary::Platforms(vec![])),
            "target-platform = []\n",
            "no platforms serialize as an empty array",
        );
        assert_eq!(
            render(mixed_platforms()),
            "[[target-platform]]\ntriple = \"x86_64-unknown-linux-gnu\"\n\
             target-features = \"unknown\"\n\n\
             [[target-platform]]\ntriple = \"x86_64-pc-windows-msvc\"\n\
             target-features = [\"sse2\"]\n",
            "several platforms serialize as an array",
        );
    }

    #[test]
    fn platform_summary_fields_round_trip() {
        #[derive(Serialize)]
        #[serde(rename_all = "kebab-case")]
        struct PlatformWrapper<T> {
            target_platform: T,
        }

        let full = PlatformSummary::new(LINUX)
            .with_custom_json("{}")
            .with_custom_cfg("target_os=\"linux\"")
            .with_target_features(sse2())
            .with_added_flags(["abc", "def"]);
        let plain = PlatformSummary::new(WINDOWS);

        let bare = toml::to_string(&PlatformWrapper {
            target_platform: &full,
        })
        .expect("platform serialized to TOML");
        let wrapper: Wrapper = toml::from_str(&bare).expect("bare platform deserialized");
        assert_eq!(
            wrapper.target_platform,
            PlatformSpecSummary::Platforms(vec![full.clone()]),
            "every PlatformSummary field survives the bare form:\n{bare}",
        );

        let list = toml::to_string(&PlatformWrapper {
            target_platform: vec![&full, &plain],
        })
        .expect("platform list serialized to TOML");
        let wrapper: Wrapper = toml::from_str(&list).expect("platform list deserialized");
        assert_eq!(
            wrapper.target_platform,
            PlatformSpecSummary::Platforms(vec![full, plain]),
            "every PlatformSummary field survives the list form:\n{list}",
        );
    }

    #[test]
    fn to_platform_spec_names_invalid_platform() {
        let err = single_platform("x86_64-unknown-foo")
            .to_platform_spec()
            .expect_err("unknown triple rejected");
        assert_eq!(err.triple(), "x86_64-unknown-foo", "error names the triple");
        assert_eq!(
            err.to_string(),
            "invalid platform `x86_64-unknown-foo`",
            "display names the triple"
        );
        assert!(
            err.source().is_some(),
            "the target-spec error is the source"
        );
        assert!(
            matches!(
                err.target_spec_error(),
                TargetSpecError::UnknownPlatformTriple(_)
            ),
            "underlying error is an unknown triple, found {:?}",
            err.target_spec_error(),
        );

        let err = PlatformSpecSummary::Platforms(vec![
            PlatformSummary::new(LINUX),
            PlatformSummary::new("x86_64-unknown-foo"),
        ])
        .to_platform_spec()
        .expect_err("unknown triple in a list rejected");
        assert_eq!(
            err.to_string(),
            "invalid platform `x86_64-unknown-foo` (element 2 of 2)",
            "display names the position within a list"
        );
    }

    #[test]
    fn to_platform_spec_accepts_non_platform_variants() {
        for summary in [PlatformSpecSummary::Always, PlatformSpecSummary::Any] {
            summary
                .to_platform_spec()
                .unwrap_or_else(|err| panic!("{summary:?} converted: {err}"));
        }
    }
}

#[cfg(all(test, feature = "proptest1"))]
mod proptests {
    use super::{tests::Wrapper, *};
    use crate::platform::Platform;
    use proptest::prelude::*;
    use std::collections::HashSet;

    fn assert_platforms_match(platform: &Platform, platform2: &Platform) {
        assert_eq!(
            platform.triple_str(),
            platform2.triple_str(),
            "triples match"
        );
        assert_eq!(
            platform.target_features(),
            platform2.target_features(),
            "target features match"
        );
        assert_eq!(
            platform.flags().collect::<HashSet<_>>(),
            platform2.flags().collect::<HashSet<_>>(),
            "flags match"
        );
    }

    proptest! {
        #[test]
        fn summary_roundtrip(platform_spec in any::<PlatformSpec>()) {
            let summary = PlatformSpecSummary::new(&platform_spec);
            let wrapper = Wrapper { target_platform: summary.clone() };
            let serialized = toml::ser::to_string(&wrapper).expect("serialization succeeded");

            let deserialized: Wrapper = toml::from_str(&serialized).expect("deserialization succeeded");
            assert_eq!(wrapper, deserialized, "summary and deserialized should match");
            let platform_spec2 = deserialized
                .target_platform
                .to_platform_spec()
                .expect("conversion to PlatformSpec succeeded");

            match (platform_spec, platform_spec2) {
                (PlatformSpec::Any, PlatformSpec::Any)
                | (PlatformSpec::Always, PlatformSpec::Always) => {},
                (PlatformSpec::Platforms(platforms), PlatformSpec::Platforms(platforms2)) => {
                    assert_eq!(platforms.len(), platforms2.len(), "platform counts match");
                    for (platform, platform2) in platforms.iter().zip(&platforms2) {
                        assert_platforms_match(platform, platform2);
                    }
                }
                (other, other2) => panic!("platform specs do not match: original: {other:?}, roundtrip: {other2:?}"),
            }
        }
    }
}
