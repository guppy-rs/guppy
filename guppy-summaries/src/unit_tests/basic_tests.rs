// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    PackageInfo, PackageMap, PackageStatus, Summary, SummaryId, SummarySource,
    diff::SummaryDiffStatus,
};
use pretty_assertions::assert_eq;
use semver::Version;
use std::collections::BTreeSet;

static SERIALIZED_SUMMARY: &str = r#"# This is a test @generated summary.

[[target-package]]
name = 'foo'
version = '1.2.3'
workspace-path = 'foo'
status = 'initial'
features = ['default', 'feature1']
optional-deps = ['dep1', 'dep2']

[[target-package]]
name = 'dep'
version = '0.4.2'
crates-io = true
status = 'direct'
features = ['std']
optional-deps = ['bar']

[[target-package]]
name = 'no-changes'
version = '1.5.3'
crates-io = true
status = 'transitive'
features = ['default']
optional-deps = ['dep2']

[[host-package]]
name = 'bar'
version = '0.1.0'
workspace-path = 'dir/bar'
status = 'workspace'
features = ['default', 'feature2']

[[host-package]]
name = 'local-dep'
version = '1.1.2'
path = '../local-dep'
status = 'transitive'
features = []
optional-deps = ['dep4']
"#;

static SUMMARY2: &str = r#"# This is a test @generated summary.

[[target-package]]
name = 'foo'
version = '1.2.3'
workspace-path = 'foo'
status = 'initial'
features = ['default', 'feature1', 'feature2']
optional-deps = ['dep1', 'dep3']

[[target-package]]
name = 'dep'
version = '0.4.3'
crates-io = true
status = 'direct'
features = ['std']
optional-deps = ['bar']

[[target-package]]
name = 'dep'
version = '0.5.0'
crates-io = true
status = 'transitive'
features = ['std']

[[target-package]]
name = 'no-changes'
version = '1.5.3'
crates-io = true
status = 'transitive'
features = ['default']
optional-deps = ['dep2']

[[host-package]]
name = 'bar'
version = '0.2.0'
workspace-path = 'dir/bar'
status = 'initial'
features = ['default', 'feature2']

[[host-package]]
name = 'local-dep'
version = '1.1.2'
path = '../local-dep'
status = 'transitive'
features = ['dep-feature']

[[host-package]]
name = 'local-dep'
version = '2.0.0'
path = '../local-dep-2'
status = 'transitive'
features = []
"#;

#[test]
fn empty_roundtrip() {
    let summary = Summary::default();

    let mut s = "# This is a test @generated summary.\n\n".to_string();
    summary.write_to_string(&mut s).expect("write succeeded");

    static SERIALIZED_SUMMARY: &str = "# This is a test @generated summary.\n\n";

    assert_eq!(&s, SERIALIZED_SUMMARY, "serialized representation matches");

    let deserialized = Summary::parse(&s).expect("from_str succeeded");
    assert_eq!(summary, deserialized, "deserialized representation matches");

    let diff = summary.diff(&deserialized);
    assert!(diff.is_unchanged(), "diff should be empty");
}

#[test]
fn basic_roundtrip() {
    let target_packages = vec![
        (
            SummaryId::new(
                "foo",
                Version::new(1, 2, 3),
                SummarySource::workspace("foo"),
            ),
            PackageStatus::Initial,
            vec!["default", "feature1"],
            vec!["dep1", "dep2"],
        ),
        (
            SummaryId::new("dep", Version::new(0, 4, 2), SummarySource::crates_io()),
            PackageStatus::Direct,
            vec!["std"],
            vec!["bar"],
        ),
        (
            SummaryId::new(
                "no-changes",
                Version::new(1, 5, 3),
                SummarySource::crates_io(),
            ),
            PackageStatus::Transitive,
            vec!["default"],
            vec!["dep2"],
        ),
    ];
    let host_packages = vec![
        (
            SummaryId::new(
                "bar",
                Version::new(0, 1, 0),
                SummarySource::workspace("dir/bar"),
            ),
            PackageStatus::Workspace,
            vec!["default", "feature2"],
            vec![],
        ),
        (
            SummaryId::new(
                "local-dep",
                Version::new(1, 1, 2),
                SummarySource::path("../local-dep"),
            ),
            PackageStatus::Transitive,
            vec![],
            vec!["dep4"],
        ),
    ];

    let summary = Summary {
        metadata: Default::default(),
        target_packages: make_summary(target_packages),
        host_packages: make_summary(host_packages),
    };

    let mut s = "# This is a test @generated summary.\n\n".to_string();
    summary.write_to_string(&mut s).expect("write succeeded");

    assert_eq!(&s, SERIALIZED_SUMMARY, "serialized representation matches");

    let deserialized = Summary::parse(&s).expect("from_str succeeded");
    assert_eq!(summary, deserialized, "deserialized representation matches");

    let diff = summary.diff(&deserialized);
    assert!(diff.is_unchanged(), "diff should be empty");

    // Try changing some things.
    let summary2 = Summary::parse(SUMMARY2).expect("from_str succeeded");
    let diff = summary.diff(&summary2);

    // target_packages is:
    // * a change for foo = 1 entry
    // * a remove + 2 inserts for dep (so it should not be combined) = 3 entries
    assert_eq!(diff.target_packages.changed.len(), 4, "4 changed entries");
    let mut iter = diff.target_packages.changed.iter();

    // First, dep 0.4.2.
    let std_feature: BTreeSet<_> = vec!["std".to_string()].into_iter().collect();
    let bar_dep: BTreeSet<_> = vec!["bar".to_string()].into_iter().collect();
    let (summary_id, status) = iter.next().expect("3 elements left");
    assert_eq!(summary_id.name, "dep");
    assert_eq!(summary_id.version.to_string(), "0.4.2");
    assert_eq!(summary_id.source, SummarySource::crates_io());
    assert_eq!(
        *status,
        SummaryDiffStatus::Removed {
            old_info: &PackageInfo {
                status: PackageStatus::Direct,
                features: std_feature.clone(),
                optional_deps: bar_dep.clone(),
            },
        },
    );

    // Next, dep 0.4.3.
    let (summary_id, status) = iter.next().expect("2 elements left");
    assert_eq!(summary_id.name, "dep");
    assert_eq!(summary_id.version.to_string(), "0.4.3");
    assert_eq!(summary_id.source, SummarySource::crates_io());
    assert_eq!(
        *status,
        SummaryDiffStatus::Added {
            info: &PackageInfo {
                status: PackageStatus::Direct,
                features: std_feature.clone(),
                optional_deps: bar_dep,
            },
        },
    );

    // Next, dep 0.5.0.
    let (summary_id, status) = iter.next().expect("1 element left");
    assert_eq!(summary_id.name, "dep");
    assert_eq!(summary_id.version.to_string(), "0.5.0");
    assert_eq!(summary_id.source, SummarySource::crates_io());
    assert_eq!(
        *status,
        SummaryDiffStatus::Added {
            info: &PackageInfo {
                status: PackageStatus::Transitive,
                features: std_feature,
                optional_deps: BTreeSet::new(),
            },
        }
    );

    // Finally, foo.
    let (summary_id, status) = iter.next().expect("0 elements left");
    assert_eq!(summary_id.name, "foo");
    assert_eq!(summary_id.version.to_string(), "1.2.3");
    assert_eq!(summary_id.source, SummarySource::workspace("foo"));
    assert_eq!(
        *status,
        SummaryDiffStatus::Modified {
            old_version: None,
            old_source: None,
            old_status: None,
            new_status: PackageStatus::Initial,
            added_features: vec!["feature2"].into_iter().collect(),
            removed_features: BTreeSet::new(),
            unchanged_features: vec!["default", "feature1"].into_iter().collect(),
            added_optional_deps: vec!["dep3"].into_iter().collect(),
            removed_optional_deps: vec!["dep2"].into_iter().collect(),
            unchanged_optional_deps: vec!["dep1"].into_iter().collect(),
        }
    );

    // host_packages is:
    // * an insert + remove for bar, so it *should* be combined = 1 entry
    // * a change + insert for local-dep, so it should not be combined = 2 entries.
    assert_eq!(diff.host_packages.changed.len(), 3, "3 changed entries");
    let mut iter = diff.host_packages.changed.iter();

    // First, bar 0.2.0.
    let (summary_id, status) = iter.next().expect("2 elements left");
    assert_eq!(summary_id.name, "bar");
    assert_eq!(summary_id.version.to_string(), "0.2.0");
    assert_eq!(summary_id.source, SummarySource::workspace("dir/bar"));
    assert_eq!(
        *status,
        SummaryDiffStatus::Modified {
            old_version: Some(&Version::new(0, 1, 0)),
            old_source: None,
            old_status: Some(PackageStatus::Workspace),
            new_status: PackageStatus::Initial,
            added_features: BTreeSet::new(),
            removed_features: BTreeSet::new(),
            unchanged_features: vec!["default", "feature2"].into_iter().collect(),
            added_optional_deps: BTreeSet::new(),
            removed_optional_deps: BTreeSet::new(),
            unchanged_optional_deps: BTreeSet::new(),
        }
    );

    // Next, local-dep 1.1.2.
    let (summary_id, status) = iter.next().expect("2 elements left");
    assert_eq!(summary_id.name, "local-dep");
    assert_eq!(summary_id.version.to_string(), "1.1.2");
    assert_eq!(summary_id.source, SummarySource::path("../local-dep"));
    assert_eq!(
        *status,
        SummaryDiffStatus::Modified {
            old_version: None,
            old_source: None,
            old_status: None,
            new_status: PackageStatus::Transitive,
            added_features: vec!["dep-feature"].into_iter().collect(),
            removed_features: BTreeSet::new(),
            unchanged_features: BTreeSet::new(),
            added_optional_deps: BTreeSet::new(),
            removed_optional_deps: vec!["dep4"].into_iter().collect(),
            unchanged_optional_deps: BTreeSet::new(),
        }
    );

    // Finally, local-dep 2.0.0.
    let (summary_id, status) = iter.next().expect("1 element left");
    assert_eq!(summary_id.name, "local-dep");
    assert_eq!(summary_id.version.to_string(), "2.0.0");
    assert_eq!(summary_id.source, SummarySource::path("../local-dep-2"));
    assert_eq!(
        *status,
        SummaryDiffStatus::Added {
            info: &PackageInfo {
                status: PackageStatus::Transitive,
                features: BTreeSet::new(),
                optional_deps: BTreeSet::new(),
            },
        },
    );
}

#[test]
fn test_serialization() {
    let summary = Summary::parse(SERIALIZED_SUMMARY).expect("from_str succeeded");
    let summary2 = Summary::parse(SUMMARY2).expect("from_str succeeded");
    let diff = summary.diff(&summary2);

    let to_serialize = &diff;

    static EXPECTED_JSON: &str = indoc::indoc!(
        r#"{
        "target-packages": {
          "changed": [
            {
              "name": "dep",
              "version": "0.4.3",
              "crates-io": true,
              "change": "added",
              "status": "direct",
              "features": [
                "std"
              ],
              "optional-deps": [
                "bar"
              ]
            },
            {
              "name": "dep",
              "version": "0.5.0",
              "crates-io": true,
              "change": "added",
              "status": "transitive",
              "features": [
                "std"
              ]
            },
            {
              "name": "foo",
              "version": "1.2.3",
              "workspace-path": "foo",
              "change": "modified",
              "old-version": null,
              "old-source": null,
              "old-status": null,
              "new-status": "initial",
              "added-features": [
                "feature2"
              ],
              "removed-features": [],
              "unchanged-features": [
                "default",
                "feature1"
              ],
              "added-optional-deps": [
                "dep3"
              ],
              "removed-optional-deps": [
                "dep2"
              ],
              "unchanged-optional-deps": [
                "dep1"
              ]
            },
            {
              "name": "dep",
              "version": "0.4.2",
              "crates-io": true,
              "change": "removed",
              "old-status": "direct",
              "old-features": [
                "std"
              ]
            }
          ],
          "unchanged": [
            {
              "name": "no-changes",
              "version": "1.5.3",
              "crates-io": true,
              "status": "transitive",
              "features": [
                "default"
              ],
              "optional-deps": [
                "dep2"
              ]
            }
          ]
        },
        "host-packages": {
          "changed": [
            {
              "name": "local-dep",
              "version": "2.0.0",
              "path": "../local-dep-2",
              "change": "added",
              "status": "transitive",
              "features": []
            },
            {
              "name": "bar",
              "version": "0.2.0",
              "workspace-path": "dir/bar",
              "change": "modified",
              "old-version": "0.1.0",
              "old-source": null,
              "old-status": "workspace",
              "new-status": "initial",
              "added-features": [],
              "removed-features": [],
              "unchanged-features": [
                "default",
                "feature2"
              ],
              "added-optional-deps": [],
              "removed-optional-deps": [],
              "unchanged-optional-deps": []
            },
            {
              "name": "local-dep",
              "version": "1.1.2",
              "path": "../local-dep",
              "change": "modified",
              "old-version": null,
              "old-source": null,
              "old-status": null,
              "new-status": "transitive",
              "added-features": [
                "dep-feature"
              ],
              "removed-features": [],
              "unchanged-features": [],
              "added-optional-deps": [],
              "removed-optional-deps": [
                "dep4"
              ],
              "unchanged-optional-deps": []
            }
          ]
        }
      }"#
    );

    let j = serde_json::to_string_pretty(&to_serialize).expect("should serialize");
    println!("json output: {j}");
    assert_eq!(j, EXPECTED_JSON);

    static EXPECTED_TOML: &str = indoc::indoc!(
        r#"[[target-packages.changed]]
    name = "dep"
    version = "0.4.3"
    crates-io = true
    change = "added"
    status = "direct"
    features = ["std"]
    optional-deps = ["bar"]

    [[target-packages.changed]]
    name = "dep"
    version = "0.5.0"
    crates-io = true
    change = "added"
    status = "transitive"
    features = ["std"]

    [[target-packages.changed]]
    name = "foo"
    version = "1.2.3"
    workspace-path = "foo"
    change = "modified"
    new-status = "initial"
    added-features = ["feature2"]
    removed-features = []
    unchanged-features = ["default", "feature1"]
    added-optional-deps = ["dep3"]
    removed-optional-deps = ["dep2"]
    unchanged-optional-deps = ["dep1"]

    [[target-packages.changed]]
    name = "dep"
    version = "0.4.2"
    crates-io = true
    change = "removed"
    old-status = "direct"
    old-features = ["std"]

    [[target-packages.unchanged]]
    name = "no-changes"
    version = "1.5.3"
    crates-io = true
    status = "transitive"
    features = ["default"]
    optional-deps = ["dep2"]

    [[host-packages.changed]]
    name = "local-dep"
    version = "2.0.0"
    path = "../local-dep-2"
    change = "added"
    status = "transitive"
    features = []

    [[host-packages.changed]]
    name = "bar"
    version = "0.2.0"
    workspace-path = "dir/bar"
    change = "modified"
    old-version = "0.1.0"
    old-status = "workspace"
    new-status = "initial"
    added-features = []
    removed-features = []
    unchanged-features = ["default", "feature2"]
    added-optional-deps = []
    removed-optional-deps = []
    unchanged-optional-deps = []

    [[host-packages.changed]]
    name = "local-dep"
    version = "1.1.2"
    path = "../local-dep"
    change = "modified"
    new-status = "transitive"
    added-features = ["dep-feature"]
    removed-features = []
    unchanged-features = []
    added-optional-deps = []
    removed-optional-deps = ["dep4"]
    unchanged-optional-deps = []
"#
    );
    let toml_out = toml::to_string(&to_serialize).expect("should serialize");
    println!("toml output: {toml_out}");
    assert_eq!(toml_out, EXPECTED_TOML);

    // TODO: add roundtrip test into the proper data structure. For now we just check that the output is valid TOML.
    let parsed = toml_out
        .parse::<toml::Table>()
        .expect("deserialization from value should work");
    println!("parsed output: {parsed:?}");
}

fn make_summary(list: Vec<(SummaryId, PackageStatus, Vec<&str>, Vec<&str>)>) -> PackageMap {
    list.into_iter()
        .map(|(summary_id, status, features, optional_deps)| {
            let features = features
                .into_iter()
                .map(|feature| feature.to_string())
                .collect();
            let optional_deps = optional_deps
                .into_iter()
                .map(|feature| feature.to_string())
                .collect();

            (
                summary_id,
                PackageInfo {
                    status,
                    features,
                    optional_deps,
                },
            )
        })
        .collect()
}

#[test]
fn metadata_round_trips_match_toml_05() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "datetime",
            "[metadata]\nd = 1979-05-27T07:32:00Z\n",
            "[metadata]\nd = 1979-05-27T07:32:00Z\n",
        ),
        (
            "array of tables declared before a plain array",
            "[metadata.omitted-packages]\nids = [{ name = 'a' }]\nworkspace-members = ['b']\n",
            "[metadata.omitted-packages]\nworkspace-members = ['b']\n\n\
             [[metadata.omitted-packages.ids]]\nname = 'a'\n",
        ),
        (
            "sub-table declared before an array of tables",
            "[metadata.o.sub]\nx = 1\n[[metadata.o.aot]]\ny = 1\n",
            "[[metadata.o.aot]]\ny = 1\n\n[metadata.o.sub]\nx = 1\n",
        ),
        (
            "scalar declared after a nested sub-table",
            "[metadata.outer.inner]\nx = 1\n[metadata.outer]\nflag = true\n",
            "[metadata.outer]\nflag = true\n\n[metadata.outer.inner]\nx = 1\n",
        ),
        (
            "misordered array of tables element",
            "[[metadata.a]]\nsub = { x = 1 }\nv = 2\n",
            "[[metadata.a]]\nv = 2\n\n[metadata.a.sub]\nx = 1\n",
        ),
        (
            "metadata alongside packages",
            "[metadata]\nname = 'n'\n[metadata.o.sub]\nx = 1\n[[metadata.o.aot]]\ny = 1\n\n\
             [[target-package]]\nname = 'foo'\nversion = '1.2.3'\nworkspace-path = 'foo'\n\
             status = 'initial'\nfeatures = ['a']\n",
            "[metadata]\nname = 'n'\n[[metadata.o.aot]]\ny = 1\n\n[metadata.o.sub]\nx = 1\n\n\
             [[target-package]]\nname = 'foo'\nversion = '1.2.3'\nworkspace-path = 'foo'\n\
             status = 'initial'\nfeatures = ['a']\n",
        ),
    ];
    for (name, input, expected) in cases {
        let summary = Summary::parse(input).unwrap_or_else(|err| panic!("{name}: parsed: {err}"));
        let actual = summary
            .to_string()
            .unwrap_or_else(|err| panic!("{name}: serialized: {err}"));
        assert_eq!(&actual, expected, "{name}");
        assert_eq!(
            Summary::parse(&actual).unwrap_or_else(|err| panic!("{name}: reparsed: {err}")),
            summary,
            "{name}: round trip"
        );
    }
}

#[test]
fn top_level_metadata_value_after_table_is_an_error() {
    // toml 0.5 did not reorder the metadata map's own entries, so this was
    // Err(ValueAfterTable) there too.
    let summary = Summary::parse("[metadata.t]\nx = 1\n[metadata]\nv = 1\n").expect("parsed");
    let err = summary.to_string().expect_err("value after table");
    assert!(
        err.to_string()
            .contains("values must be emitted before tables"),
        "unexpected error: {err}"
    );
}

#[test]
fn with_metadata_preserves_datetimes_and_reorders_fields() {
    #[derive(serde::Serialize)]
    struct Metadata {
        nested: Nested,
        when: toml::value::Datetime,
        name: &'static str,
        items: Vec<Nested>,
    }

    #[derive(serde::Serialize)]
    struct Nested {
        x: i64,
        missing: Option<u32>,
    }

    let metadata = Metadata {
        nested: Nested {
            x: 1,
            missing: None,
        },
        when: "1979-05-27T07:32:00Z".parse().expect("valid datetime"),
        name: "n",
        items: vec![Nested {
            x: 2,
            missing: None,
        }],
    };
    let summary = Summary::with_metadata(&metadata).expect("metadata serialized");
    assert!(
        summary.metadata["when"].is_datetime(),
        "datetime preserved: {:?}",
        summary.metadata["when"]
    );

    let actual = summary.to_string().expect("summary serialized");
    assert_eq!(
        actual,
        "[metadata]\nwhen = 1979-05-27T07:32:00Z\nname = 'n'\n\n[metadata.nested]\nx = 1\n\n\
         [[metadata.items]]\nx = 2\n"
    );
    assert_eq!(
        Summary::parse(&actual).expect("reparsed"),
        summary,
        "round trip"
    );
}
