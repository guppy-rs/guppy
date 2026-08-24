// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixtures::json::JsonFixture;
use guppy::{
    Error, Version,
    graph::{
        PackageGraph,
        cargo::{CargoOptions, CargoSet},
        feature::{FeatureSet, StandardFeatures},
        summaries::{CargoSetInputsSummary, Summary, SummaryId, SummarySource},
    },
};
use pretty_assertions::assert_eq;

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
    let summary = cargo_set.to_summary(&opts).expect("summary generated");

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
        rebuilt
            .to_summary(&inputs.options)
            .expect("rebuilt summary generated"),
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
features = ['summaries', 'no-such-feature']
optional-deps = ['guppy-summaries', 'no-such-dep']

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
        .expect_err("unknown features-only elements rejected");

    #[cfg_attr(guppy_nightly, expect(non_exhaustive_omitted_patterns))]
    match &err {
        Error::UnknownFeaturesOnlySummary {
            unknown_summary_ids,
            unknown_features,
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
        }
        other => panic!("expected UnknownFeaturesOnlySummary, found {other:?}"),
    }

    assert_eq!(
        err.to_string(),
        "\
unknown elements: resolving features-only
* unknown summary IDs:
  - { name = \"not-a-member\", version = \"0.1.0\", source = \"path 'not-a-member'\"}
  - { name = \"no-such-crate\", version = \"1.0.0\", source = \"crates.io\"}
* unknown features:
  - { name = \"guppy\", version = \"0.5.0\", source = \"path 'guppy'\"}:
    - features: no-such-feature
    - optional-deps: no-such-dep
",
        "error message lists every unknown element",
    );
}
