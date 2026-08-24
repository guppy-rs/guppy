// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::context::ContextImpl;
use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use fixtures::json::JsonFixture;
use guppy::graph::{
    cargo::CargoSet,
    summaries::{CargoSetInputsSummary, Summary, diff::SummaryDiff},
};
use guppy_cmdlib::PackagesAndFeatures;
use hakari::diffy::{PatchFormatter, create_patch};
use once_cell::sync::Lazy;
use proptest_ext::ValueGenerator;
use std::fmt::Write;

pub struct SummaryContext;

pub struct ExistingSummary {
    summary: Summary,
    contents: String,
}

impl<'g> ContextImpl<'g> for SummaryContext {
    type IterArgs = usize;
    type IterItem = (usize, Summary);
    type Existing = ExistingSummary;

    fn dir_name(fixture: &'g JsonFixture) -> Utf8PathBuf {
        fixture
            .abs_path()
            .parent()
            .expect("up to dirname of summary")
            .join("summaries")
    }

    fn file_name(fixture: &'g JsonFixture, &(count, _): &Self::IterItem) -> String {
        format!("{}-{}.toml", fixture.name(), count)
    }

    fn iter(
        fixture: &'g JsonFixture,
        &count: &Self::IterArgs,
    ) -> Box<dyn Iterator<Item = Self::IterItem> + 'g> {
        // Make a fresh generator for each summary so that filtering by --fixtures continues to
        // produce deterministic results.
        let mut generator = ValueGenerator::from_seed(fixture.name());

        let graph = fixture.graph();

        let packages_features_strategy = PackagesAndFeatures::strategy(graph);
        let cargo_opts_strategy = graph.proptest1_cargo_options_strategy();

        let iter = (0..count).map(move |idx| {
            // The partial clones mean that e.g. a change to the algorithm in
            // packages_features_strategy won't affect generation of cargo_opts.
            let mut iter_generator = generator.partial_clone();

            let packages_features = iter_generator
                .partial_clone()
                .generate(&packages_features_strategy);
            let (initials, features_only) = packages_features
                .make_feature_sets(graph)
                .expect("valid feature set");

            let cargo_opts = iter_generator
                .partial_clone()
                .generate(&cargo_opts_strategy);
            let cargo_set = CargoSet::new(initials, features_only, &cargo_opts)
                .expect("into_cargo_set succeeded");
            let summary = cargo_set
                .to_summary()
                .expect("generated summaries should serialize correctly");

            let metadata: CargoSetInputsSummary = summary
                .metadata
                .clone()
                .try_into()
                .expect("metadata deserialized as a CargoSetInputsSummary");
            let inputs = metadata
                .to_cargo_set_inputs(graph)
                .expect("cargo set inputs rebuilt from the summary");
            assert_eq!(
                &inputs.features_only,
                &cargo_set.inputs().features_only,
                "features-only set rebuilt from the summary",
            );
            let rebuilt_summary = inputs
                .to_cargo_set(cargo_set.initials().clone())
                .expect("cargo set rebuilt from the summary")
                .to_summary()
                .expect("rebuilt summary generated");
            assert_eq!(
                rebuilt_summary, summary,
                "resolution rebuilt from the summary matches the original",
            );

            (idx, summary)
        });

        Box::new(iter)
    }

    fn parse_existing(_: &Utf8Path, contents: String) -> Result<Self::Existing> {
        let summary = Summary::parse(&contents)?;
        Ok(ExistingSummary { summary, contents })
    }

    fn is_changed(
        fixture: &'g JsonFixture,
        item: &Self::IterItem,
        existing: &Self::Existing,
    ) -> Result<bool> {
        let mut rendered = String::new();
        Self::write_to_string(fixture, item, &mut rendered)?;
        Ok(existing.contents != rendered)
    }

    fn diff(
        fixture: &'g JsonFixture,
        item @ (_, summary): &Self::IterItem,
        existing: Option<&Self::Existing>,
    ) -> String {
        // Need to make this a static to allow lifetimes to work out.
        static EMPTY_SUMMARY: Lazy<Summary> = Lazy::new(Summary::default);

        let existing_summary = match existing {
            Some(existing) => &existing.summary,
            None => &*EMPTY_SUMMARY,
        };

        let diff = SummaryDiff::new(existing_summary, summary);
        if diff.is_changed() {
            return format!("{}", diff.report());
        }

        let existing_contents = existing.map_or("", |existing| existing.contents.as_str());
        let mut rendered = String::new();
        match Self::write_to_string(fixture, item, &mut rendered) {
            Ok(()) => {
                let patch = create_patch(existing_contents, &rendered);
                format!("{}", PatchFormatter::new().fmt_patch(&patch))
            }
            Err(err) => format!("error while rendering summary: {err}"),
        }
    }

    fn write_to_string(
        fixture: &'g JsonFixture,
        (_, summary): &Self::IterItem,
        out: &mut String,
    ) -> Result<()> {
        writeln!(
            out,
            "# This summary was @generated. To regenerate, run:\n\
             #   cargo run -p fixture-manager -- generate-summaries --fixture {}\n",
            fixture.name()
        )?;

        summary.write_to_string(out)?;
        Ok(())
    }
}
