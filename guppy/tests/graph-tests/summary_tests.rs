// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixtures::json::JsonFixture;
use guppy::{
    Error, Version,
    graph::{
        PackageGraph,
        cargo::{CargoOptions, CargoSet, CargoSetInputs},
        feature::{FeatureId, FeatureSet, StandardFeatures},
        summaries::{CargoSetInputsSummary, Summary, SummaryId, SummarySource},
    },
};
use pretty_assertions::assert_eq;

#[test]
fn features_only_summary_records_missing_base() {
    let graph = JsonFixture::metadata_guppy_c9b4f76().graph();
    let guppy_id = graph
        .workspace()
        .member_by_name("guppy")
        .expect("guppy is a workspace member")
        .id();
    let features_only = graph
        .feature_graph()
        .resolve_ids([FeatureId::named(guppy_id, "summaries")])
        .expect("summaries is a feature of guppy");
    let inputs = CargoSetInputs::new(CargoOptions::new(), features_only.clone());

    let summary = CargoSetInputsSummary::new(&inputs).expect("inputs summary generated");
    let entry = match summary.features_only.as_slice() {
        [entry] => entry,
        other => panic!("expected exactly one features-only entry, found {other:?}"),
    };
    assert!(!entry.base, "missing base feature recorded in the summary");

    let serialized = toml::to_string(&summary).expect("summary serialized to TOML");
    assert!(
        serialized.contains("base = false"),
        "serialized summary records the missing base: {serialized}"
    );
    let parsed: CargoSetInputsSummary =
        toml::from_str(&serialized).expect("summary parsed from TOML");
    assert_eq!(parsed, summary, "summary round-tripped through TOML");

    let rebuilt = parsed
        .to_cargo_set_inputs(graph)
        .expect("cargo set inputs rebuilt from the summary");
    assert_eq!(
        rebuilt.features_only, features_only,
        "features-only set rebuilt without the base feature",
    );
}

fn feature_sets_for_guppy_c9b4f76(graph: &PackageGraph) -> (FeatureSet<'_>, FeatureSet<'_>) {
    let initials = graph
        .resolve_workspace_names(["fixtures"])
        .expect("fixtures is a workspace member")
        .to_feature_set(StandardFeatures::Default);
    let features_only = graph
        .resolve_workspace_names(["guppy"])
        .expect("guppy is a workspace member")
        .to_feature_set(StandardFeatures::All);
    (initials, features_only)
}

#[test]
fn features_only_changes_resolution() {
    let graph = JsonFixture::metadata_guppy_c9b4f76().graph();
    let (initials, features_only) = feature_sets_for_guppy_c9b4f76(graph);
    let opts = CargoOptions::new();

    let with_features_only = CargoSet::new(initials.clone(), features_only, &opts)
        .expect("cargo set built with a features-only set");
    let without_features_only =
        CargoSet::new(initials, graph.feature_graph().resolve_none(), &opts)
            .expect("cargo set built without a features-only set");

    assert_ne!(
        with_features_only.target_features(),
        without_features_only.target_features(),
        "features-only set alters the resolution",
    );
}

#[test]
fn features_only_summary_round_trip() {
    let graph = JsonFixture::metadata_guppy_c9b4f76().graph();
    let (initials, features_only) = feature_sets_for_guppy_c9b4f76(graph);
    let opts = CargoOptions::new();

    let cargo_set = CargoSet::new(initials.clone(), features_only.clone(), &opts)
        .expect("cargo set built with a features-only set");
    let summary = cargo_set.to_summary().expect("summary generated");

    let serialized = summary.to_string().expect("summary serialized to TOML");
    let parsed = Summary::parse(&serialized).expect("summary parsed from TOML");
    assert_eq!(parsed, summary, "summary round-tripped through TOML");

    let metadata: CargoSetInputsSummary = parsed
        .metadata
        .try_into()
        .expect("metadata deserialized as a CargoSetInputsSummary");
    let inputs = metadata
        .to_cargo_set_inputs(graph)
        .expect("cargo set inputs rebuilt from the summary");

    assert_eq!(
        inputs.features_only, features_only,
        "features-only set rebuilt from the summary",
    );

    let rebuilt = inputs
        .to_cargo_set(initials)
        .expect("cargo set rebuilt from the summary");
    assert_eq!(
        rebuilt.to_summary().expect("rebuilt summary generated"),
        summary,
        "resolution rebuilt from the summary matches the original",
    );
}

#[test]
fn features_only_summary_unknown_elements() {
    let graph = JsonFixture::metadata_guppy_c9b4f76().graph();
    let metadata = "\
resolver = '2'
include-dev = false
initials-platform = 'standard'

[[features-only]]
name = 'guppy'
version = '0.5.0'
workspace-path = 'guppy'
base = false
features = ['summaries', 'no-such-feature']
optional-deps = ['guppy-summaries', 'no-such-dep']

[[features-only]]
name = 'fixtures'
version = '0.1.0'
workspace-path = 'fixtures'
base = false
features = []

[[features-only]]
name = 'not-a-member'
version = '0.1.0'
workspace-path = 'not-a-member'
features = []

[[features-only]]
name = 'no-such-crate'
version = '1.0.0'
crates-io = true
features = []
";

    let summary: CargoSetInputsSummary =
        toml::from_str(metadata).expect("summary parsed from TOML");
    let err = summary
        .to_cargo_set_inputs(graph)
        .expect_err("invalid features-only entries rejected");

    #[cfg_attr(guppy_nightly, expect(non_exhaustive_omitted_patterns))]
    match &err {
        Error::InvalidFeaturesOnlySummary {
            unknown_summary_ids,
            unknown_features,
            empty_entries,
        } => {
            assert_eq!(
                unknown_summary_ids,
                &[
                    SummaryId::new(
                        "not-a-member",
                        Version::new(0, 1, 0),
                        SummarySource::workspace("not-a-member"),
                    ),
                    SummaryId::new(
                        "no-such-crate",
                        Version::new(1, 0, 0),
                        SummarySource::crates_io(),
                    ),
                ],
                "every unknown package is reported",
            );
            let entry = match unknown_features.as_slice() {
                [entry] => entry,
                other => panic!("expected exactly one unknown-features entry, found {other:?}"),
            };
            assert_eq!(
                entry.summary_id,
                SummaryId::new(
                    "guppy",
                    Version::new(0, 5, 0),
                    SummarySource::workspace("guppy")
                ),
            );
            assert_eq!(
                entry.features.iter().collect::<Vec<_>>(),
                ["no-such-feature"],
                "only the unknown named features are reported",
            );
            assert_eq!(
                entry.optional_deps.iter().collect::<Vec<_>>(),
                ["no-such-dep"],
                "only the unknown optional deps are reported",
            );
            assert_eq!(
                empty_entries,
                &[SummaryId::new(
                    "fixtures",
                    Version::new(0, 1, 0),
                    SummarySource::workspace("fixtures"),
                )],
                "the entry that enables nothing is reported",
            );
        }
        other => panic!("expected InvalidFeaturesOnlySummary, found {other:?}"),
    }

    assert_eq!(
        err.to_string(),
        "\
invalid entries: resolving features-only
* unknown summary IDs:
  - { name = \"not-a-member\", version = \"0.1.0\", source = \"path 'not-a-member'\"}
  - { name = \"no-such-crate\", version = \"1.0.0\", source = \"crates.io\"}
* unknown features:
  - { name = \"guppy\", version = \"0.5.0\", source = \"path 'guppy'\"}:
    - features: no-such-feature
    - optional-deps: no-such-dep
* entries that enable nothing (base = false, no features):
  - { name = \"fixtures\", version = \"0.1.0\", source = \"path 'fixtures'\"}
",
        "error message lists every invalid entry",
    );
}

#[test]
fn features_only_summary_rejects_empty_entry() {
    let graph = JsonFixture::metadata_guppy_c9b4f76().graph();
    let metadata = "\
resolver = '2'
include-dev = false
initials-platform = 'standard'

[[features-only]]
name = 'fixtures'
version = '0.1.0'
workspace-path = 'fixtures'
base = false
features = []
";

    let summary: CargoSetInputsSummary =
        toml::from_str(metadata).expect("summary parsed from TOML");
    let entry = match summary.features_only.as_slice() {
        [entry] => entry,
        other => panic!("expected exactly one features-only entry, found {other:?}"),
    };
    assert!(
        entry.is_empty(),
        "entry with base = false and no features is empty"
    );

    let err = summary
        .to_cargo_set_inputs(graph)
        .expect_err("entry that enables nothing rejected");

    #[cfg_attr(guppy_nightly, expect(non_exhaustive_omitted_patterns))]
    match &err {
        Error::InvalidFeaturesOnlySummary {
            unknown_summary_ids,
            unknown_features,
            empty_entries,
        } => {
            assert!(unknown_summary_ids.is_empty(), "the package is known");
            assert!(unknown_features.is_empty(), "no features to be unknown");
            assert_eq!(
                empty_entries,
                &[SummaryId::new(
                    "fixtures",
                    Version::new(0, 1, 0),
                    SummarySource::workspace("fixtures"),
                )],
                "the empty entry is reported rather than silently dropped",
            );
        }
        other => panic!("expected InvalidFeaturesOnlySummary, found {other:?}"),
    }
}
