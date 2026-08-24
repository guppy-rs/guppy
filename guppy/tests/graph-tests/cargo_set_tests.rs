// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use fixtures::json::JsonFixture;
use guppy::graph::{
    DependencyDirection, PackageLink,
    cargo::{
        BuildPlatform, CargoLinkContext, CargoLinkVisitor, CargoOptions, CargoResolverVersion,
        CargoSet,
    },
    feature::StandardFeatures,
};
use std::collections::HashSet;

struct CargoLinkVisitorForTesting<'a, 'g> {
    /// Optional filter of `link`s.  If `None`, then all links are accepted.
    link_filter: Option<&'a dyn Fn(PackageLink<'g>) -> bool>,

    /// The value of `CargoOptions::set_include_dev` the visitor expects to be
    /// used with, for checking `CargoLinkContext::considers_dev_deps`.
    include_dev: bool,

    /// The `trace` field stores `link`s that were passed to `fn visit_link`.
    /// The links are formatted as `"foo@1.2.3 => bar@4.5.6"`.
    /// The links are stored in the order of `fn visit_link` calls.
    trace: Vec<String>,

    /// Like `trace`, but also records the `BuildPlatform` each link was
    /// visited on.
    platform_trace: Vec<(BuildPlatform, String)>,
}

impl<'a, 'g> CargoLinkVisitorForTesting<'a, 'g> {
    fn new() -> Self {
        Self {
            link_filter: None,
            include_dev: false,
            trace: vec![],
            platform_trace: vec![],
        }
    }

    fn with_filter(f: &'a impl Fn(PackageLink<'g>) -> bool) -> Self {
        Self {
            link_filter: Some(f),
            include_dev: false,
            trace: vec![],
            platform_trace: vec![],
        }
    }

    fn with_include_dev(include_dev: bool) -> Self {
        Self {
            link_filter: None,
            include_dev,
            trace: vec![],
            platform_trace: vec![],
        }
    }
}

fn link_to_string(link: &PackageLink) -> String {
    format!(
        "{}@{} => {}@{}",
        link.from().name(),
        link.from().version(),
        link.to().name(),
        link.to().version(),
    )
}

fn links_to_strings<'g>(links: impl IntoIterator<Item = PackageLink<'g>>) -> Vec<String> {
    let mut result = links
        .into_iter()
        .map(|link| link_to_string(&link))
        .collect::<Vec<_>>();
    result.sort();
    result
}

impl<'g> CargoLinkVisitor<'g> for CargoLinkVisitorForTesting<'_, 'g> {
    fn visit_link(&mut self, cx: &CargoLinkContext<'_, 'g>, link: PackageLink<'g>) -> bool {
        let link_str = link_to_string(&link);

        // The context must return values consistent with what can be computed
        // from the link and options directly.
        assert_eq!(
            cx.considers_build_deps(&link),
            link.from().has_build_script(),
            "considers_build_deps for {link_str}",
        );
        assert_eq!(
            cx.considers_dev_deps(&link),
            self.include_dev && cx.package_context().starts_from_initial(&link),
            "considers_dev_deps for {link_str}",
        );
        assert_eq!(
            cx.package_context().direction(),
            DependencyDirection::Forward,
            "direction for {link_str}",
        );
        match cx.build_platform() {
            BuildPlatform::Target => {}
            BuildPlatform::Host => {
                assert_eq!(
                    format!("{:?}", cx.platform_spec()),
                    format!("{:?}", cx.build_dep_platform_spec()),
                    "host pass evaluates all deps against the host platform for {link_str}",
                );
            }
        }

        self.trace.push(link_str.clone());
        self.platform_trace.push((cx.build_platform(), link_str));
        self.link_filter.map(|f| f(link)).unwrap_or(true)
    }
}

fn cargo_set_with_visitor<'g>(
    test_fixture: &'g JsonFixture,
    root_package_name: &str,
    visitor: &mut dyn CargoLinkVisitor<'g>,
) -> CargoSet<'g> {
    let cargo_options = CargoOptions::new();
    cargo_set_with_visitor_and_options(test_fixture, root_package_name, visitor, &cargo_options)
}

fn cargo_set_with_visitor_and_options<'g>(
    test_fixture: &'g JsonFixture,
    root_package_name: &str,
    visitor: &mut dyn CargoLinkVisitor<'g>,
    cargo_options: &CargoOptions<'_>,
) -> CargoSet<'g> {
    let package_graph = test_fixture.graph();

    let initials = package_graph
        .resolve_package_name(root_package_name)
        .to_feature_set(StandardFeatures::Default);
    let no_extra_features = package_graph
        .resolve_none()
        .to_feature_set(StandardFeatures::Default);

    CargoSet::with_cargo_link_visitor(initials, no_extra_features, visitor, cargo_options).unwrap()
}

/// Returns the links from `platform_trace` visited on `build_platform`,
/// sorted.
fn platform_trace_links(
    platform_trace: &[(BuildPlatform, String)],
    build_platform: BuildPlatform,
) -> Vec<String> {
    let mut result = platform_trace
        .iter()
        .filter(|(platform, _)| *platform == build_platform)
        .map(|(_, link)| link.clone())
        .collect::<Vec<_>>();
    result.sort();
    result
}

fn cargo_set_package_names(cargo_set: &CargoSet) -> Vec<String> {
    let mut result = cargo_set
        .target_features()
        .union(cargo_set.host_features())
        .packages_with_features(DependencyDirection::Forward)
        .map(|feature_list| feature_list.package().name().to_string())
        .collect::<Vec<_>>();
    result.sort();
    result
}

// Resolver versions 2 and 3 visit this link, but `region` is only enabled as a
// dev dependency.
const VISITED_BUT_NOT_ENABLED: &str = "datatest@0.4.2 => region@2.1.2";

// testcrate turns on datatest/unsafe_test_runner via a dev dependency.
const REGION_SUBTREE: [&str; 4] = ["bitflags", "libc", "mach", "region"];

#[test]
fn test_default_resolver_is_v3() {
    let mut v3_visitor = CargoLinkVisitorForTesting::new();
    let v3_set = cargo_set_with_visitor_and_options(
        JsonFixture::metadata1(),
        "testcrate",
        &mut v3_visitor,
        &CargoOptions::new(),
    );
    let v3_names = cargo_set_package_names(&v3_set)
        .into_iter()
        .collect::<HashSet<_>>();
    for name in REGION_SUBTREE {
        assert!(
            !v3_names.contains(name),
            "{name} is absent under the default resolver (v3 resolves \
             features identically to v2, and doesn't unify dev-dependency \
             features into a non-dev build)",
        );
    }

    let mut v1_options = CargoOptions::new();
    v1_options.set_resolver(CargoResolverVersion::V1);
    let mut v1_visitor = CargoLinkVisitorForTesting::new();
    let v1_set = cargo_set_with_visitor_and_options(
        JsonFixture::metadata1(),
        "testcrate",
        &mut v1_visitor,
        &v1_options,
    );
    let v1_names = cargo_set_package_names(&v1_set)
        .into_iter()
        .collect::<HashSet<_>>();
    for name in REGION_SUBTREE {
        assert!(
            v1_names.contains(name),
            "{name} is present under resolver v1, which unifies dev-dependency \
             features into a non-dev build",
        );
    }
}

#[test]
fn test_package_link_visitor_visits() {
    let mut visitor = CargoLinkVisitorForTesting::new();
    let cargo_set = cargo_set_with_visitor(JsonFixture::metadata1(), "testcrate", &mut visitor);
    assert_eq!(
        cargo_set_package_names(&cargo_set),
        vec![
            "aho-corasick",
            "ctor",
            "datatest",
            "datatest-derive",
            "dtoa",
            "lazy_static",
            "linked-hash-map",
            "memchr",
            "proc-macro2",
            "quote",
            "regex",
            "regex-syntax",
            "same-file",
            "serde",
            "serde_yaml",
            "syn",
            "testcrate",
            "thread_local",
            "unicode-xid",
            "version_check",
            "walkdir",
            "winapi",
            "winapi-i686-pc-windows-gnu",
            "winapi-util",
            "winapi-x86_64-pc-windows-gnu",
            "yaml-rust",
        ],
    );
    assert_eq!(
        visitor.trace,
        vec![
            "testcrate@0.1.0 => datatest@0.4.2",
            "datatest@0.4.2 => yaml-rust@0.4.3",
            "datatest@0.4.2 => walkdir@2.2.9",
            "datatest@0.4.2 => version_check@0.9.1",
            "datatest@0.4.2 => serde_yaml@0.8.9",
            "datatest@0.4.2 => serde@1.0.100",
            "datatest@0.4.2 => region@2.1.2",
            "datatest@0.4.2 => regex@1.3.1",
            "datatest@0.4.2 => datatest-derive@0.4.0",
            "datatest@0.4.2 => ctor@0.1.10",
            "regex@1.3.1 => thread_local@0.3.6",
            "regex@1.3.1 => regex-syntax@0.6.12",
            "regex@1.3.1 => memchr@2.2.1",
            "regex@1.3.1 => aho-corasick@0.7.6",
            "aho-corasick@0.7.6 => memchr@2.2.1",
            "thread_local@0.3.6 => lazy_static@1.4.0",
            "serde_yaml@0.8.9 => yaml-rust@0.4.3",
            "serde_yaml@0.8.9 => serde@1.0.100",
            "serde_yaml@0.8.9 => linked-hash-map@0.5.2",
            "serde_yaml@0.8.9 => dtoa@0.4.4",
            "yaml-rust@0.4.3 => linked-hash-map@0.5.2",
            "walkdir@2.2.9 => winapi-util@0.1.2",
            "walkdir@2.2.9 => winapi@0.3.8",
            "walkdir@2.2.9 => same-file@1.0.5",
            "same-file@1.0.5 => winapi-util@0.1.2",
            "winapi-util@0.1.2 => winapi@0.3.8",
            "winapi@0.3.8 => winapi-x86_64-pc-windows-gnu@0.4.0",
            "winapi@0.3.8 => winapi-i686-pc-windows-gnu@0.4.0",
            "ctor@0.1.10 => syn@1.0.5",
            "ctor@0.1.10 => quote@1.0.2",
            "quote@1.0.2 => proc-macro2@1.0.3",
            "proc-macro2@1.0.3 => unicode-xid@0.2.0",
            "syn@1.0.5 => unicode-xid@0.2.0",
            "syn@1.0.5 => quote@1.0.2",
            "syn@1.0.5 => proc-macro2@1.0.3",
            "datatest-derive@0.4.0 => syn@1.0.5",
            "datatest-derive@0.4.0 => quote@1.0.2",
            "datatest-derive@0.4.0 => proc-macro2@1.0.3",
        ],
    );

    let mut expected_trace = links_to_strings(
        cargo_set
            .proc_macro_links()
            .chain(cargo_set.build_dep_links())
            .chain(cargo_set.target_links())
            .chain(cargo_set.host_links()),
    );
    expected_trace.push(VISITED_BUT_NOT_ENABLED.to_owned());
    expected_trace.sort();
    let mut sorted_trace = visitor.trace.clone();
    sorted_trace.sort();
    assert_eq!(sorted_trace, expected_trace);

    assert_eq!(
        links_to_strings(cargo_set.proc_macro_links()),
        vec![
            "datatest@0.4.2 => ctor@0.1.10",
            "datatest@0.4.2 => datatest-derive@0.4.0",
        ],
    );
    assert_eq!(
        links_to_strings(cargo_set.build_dep_links()),
        vec!["datatest@0.4.2 => version_check@0.9.1",],
    );
    assert_eq!(
        links_to_strings(cargo_set.target_links()),
        vec![
            "aho-corasick@0.7.6 => memchr@2.2.1",
            "datatest@0.4.2 => regex@1.3.1",
            "datatest@0.4.2 => serde@1.0.100",
            "datatest@0.4.2 => serde_yaml@0.8.9",
            "datatest@0.4.2 => walkdir@2.2.9",
            "datatest@0.4.2 => yaml-rust@0.4.3",
            "regex@1.3.1 => aho-corasick@0.7.6",
            "regex@1.3.1 => memchr@2.2.1",
            "regex@1.3.1 => regex-syntax@0.6.12",
            "regex@1.3.1 => thread_local@0.3.6",
            "same-file@1.0.5 => winapi-util@0.1.2",
            "serde_yaml@0.8.9 => dtoa@0.4.4",
            "serde_yaml@0.8.9 => linked-hash-map@0.5.2",
            "serde_yaml@0.8.9 => serde@1.0.100",
            "serde_yaml@0.8.9 => yaml-rust@0.4.3",
            "testcrate@0.1.0 => datatest@0.4.2",
            "thread_local@0.3.6 => lazy_static@1.4.0",
            "walkdir@2.2.9 => same-file@1.0.5",
            "walkdir@2.2.9 => winapi-util@0.1.2",
            "walkdir@2.2.9 => winapi@0.3.8",
            "winapi-util@0.1.2 => winapi@0.3.8",
            "winapi@0.3.8 => winapi-i686-pc-windows-gnu@0.4.0",
            "winapi@0.3.8 => winapi-x86_64-pc-windows-gnu@0.4.0",
            "yaml-rust@0.4.3 => linked-hash-map@0.5.2",
        ],
    );
    assert_eq!(
        links_to_strings(cargo_set.host_links()),
        vec![
            "ctor@0.1.10 => quote@1.0.2",
            "ctor@0.1.10 => syn@1.0.5",
            "datatest-derive@0.4.0 => proc-macro2@1.0.3",
            "datatest-derive@0.4.0 => quote@1.0.2",
            "datatest-derive@0.4.0 => syn@1.0.5",
            "proc-macro2@1.0.3 => unicode-xid@0.2.0",
            "quote@1.0.2 => proc-macro2@1.0.3",
            "syn@1.0.5 => proc-macro2@1.0.3",
            "syn@1.0.5 => quote@1.0.2",
            "syn@1.0.5 => unicode-xid@0.2.0",
        ],
    );
}

#[test]
fn test_cargo_link_visitor_build_platforms() {
    let mut visitor = CargoLinkVisitorForTesting::new();
    let cargo_set = cargo_set_with_visitor(JsonFixture::metadata1(), "testcrate", &mut visitor);

    // testcrate has proc-macro and build deps, so both passes show up in the
    // trace.
    assert!(
        !platform_trace_links(&visitor.platform_trace, BuildPlatform::Host).is_empty(),
        "host pass visited at least one link",
    );
    assert!(
        !platform_trace_links(&visitor.platform_trace, BuildPlatform::Target).is_empty(),
        "target pass visited at least one link",
    );

    // The target trace also has visited-but-not-enabled links.
    let mut expected_target = links_to_strings(
        cargo_set
            .target_links()
            .chain(cargo_set.proc_macro_links())
            .chain(cargo_set.build_dep_links()),
    );
    expected_target.push(VISITED_BUT_NOT_ENABLED.to_owned());
    expected_target.sort();
    assert_eq!(
        platform_trace_links(&visitor.platform_trace, BuildPlatform::Target),
        expected_target,
    );
    assert_eq!(
        platform_trace_links(&visitor.platform_trace, BuildPlatform::Host),
        links_to_strings(cargo_set.host_links()),
    );
}

#[test]
fn test_cargo_link_visitor_include_dev() {
    let mut visitor = CargoLinkVisitorForTesting::with_include_dev(true);
    let mut cargo_options = CargoOptions::new();
    cargo_options.set_include_dev(true);
    let cargo_set = cargo_set_with_visitor_and_options(
        JsonFixture::metadata1(),
        "testcrate",
        &mut visitor,
        &cargo_options,
    );

    let host_links = platform_trace_links(&visitor.platform_trace, BuildPlatform::Host);
    let target_links = platform_trace_links(&visitor.platform_trace, BuildPlatform::Target);
    assert!(!host_links.is_empty());
    assert!(!target_links.is_empty());
    assert_eq!(
        platform_trace_links(&visitor.platform_trace, BuildPlatform::Host),
        links_to_strings(cargo_set.host_links()),
    );
}

#[test]
fn test_package_link_visitor_filtering_normal_links_on_target() {
    let mut visitor = CargoLinkVisitorForTesting::with_filter(&|link| {
        // Remove `winapi` and `winapu-util` links.  This should transitively remove `winapi =>
        // winapi-x86_64-pc-windows-gnu` and `winapi => winapi-i686-pc-windows-gnu`.
        //
        // This filter is meant to test whether `CargoSet` algotithm consults the `visitor`
        // in all required cases.  The filter may or may not make sense in practice (here we
        // can pretend that we are filtering all packages that are only needed on Windows).
        !link.to().name().starts_with("winapi")
    });
    let cargo_set = cargo_set_with_visitor(JsonFixture::metadata1(), "testcrate", &mut visitor);

    // No `winapi...` packages (unlike in `test_package_link_visitor_visits`).
    let package_names = cargo_set_package_names(&cargo_set)
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!package_names.contains("winapi"));
    assert!(!package_names.contains("winapi-util"));

    // No `winapi...` => ... links (unlike in `test_package_link_visitor_visits`).
    let trace = visitor.trace.into_iter().collect::<HashSet<_>>();
    assert!(!trace.contains("winapi@0.3.8 => winapi-x86_64-pc-windows-gnu@0.4.0"));
    assert!(!trace.contains("winapi@0.3.8 => winapi-i686-pc-windows-gnu@0.4.0"));
    assert!(!trace.contains("winapi-util@0.1.2 => winapi@0.3.8"));

    // The visitor was asked about these links, but didn't accept them.
    // Therefore these links should be present in the `trace`, but missing from
    // the final `cargo_set`.
    let cargo_set_links = links_to_strings(cargo_set.target_links())
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!cargo_set_links.contains("walkdir@2.2.9 => winapi@0.3.8"));
    assert!(!cargo_set_links.contains("same-file@1.0.5 => winapi-util@0.1.2"));
    assert!(trace.contains("walkdir@2.2.9 => winapi@0.3.8"));
    assert!(trace.contains("same-file@1.0.5 => winapi-util@0.1.2"));
}

#[test]
fn test_package_link_visitor_filtering_build_links_on_target() {
    let mut visitor = CargoLinkVisitorForTesting::with_filter(&|link| {
        // Remove `datatest` => `version_check` build dependency.
        //
        // This filter is meant to test whether `CargoSet` algotithm consults the `visitor`
        // in all required cases.  The filter may or may not make sense in practice (here
        // the trimmed down graph would fail to build...).
        link.to().name() != "version_check"
    });
    let cargo_set = cargo_set_with_visitor(JsonFixture::metadata1(), "testcrate", &mut visitor);

    // No `version_check...` packages (unlike in `test_package_link_visitor_visits`).
    let package_names = cargo_set_package_names(&cargo_set)
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!package_names.contains("version_check"));

    // If `version_check` has transitive dependencies, then we would test here that
    // they were not visited/consulted by the `visitor`.

    // The visitor was asked about these links, but didn't accept them.
    // Therefore these links should be present in the `trace`, but missing from
    // the final `cargo_set`.
    let trace = visitor.trace.into_iter().collect::<HashSet<_>>();
    let cargo_set_links = links_to_strings(cargo_set.build_dep_links())
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!cargo_set_links.contains("datatest@0.4.2 => version_check@0.9.1"));
    dbg!(&trace);
    assert!(trace.contains("datatest@0.4.2 => version_check@0.9.1"));
}

#[test]
fn test_package_link_visitor_filtering_links_on_host() {
    let mut visitor = CargoLinkVisitorForTesting::with_filter(&|link| {
        // Remove dependencies of `ctor` and `datatest-derive` packages.  This should transitively
        // remove `proc-macro2`, `quote`, `syn`, and `unicode-xid` packages.
        //
        // This filter is meant to test whether `CargoSet` algotithm consults the `visitor`
        // in all required cases.  The filter may or may not make sense in practice (here
        // the trimmed down graph would fail to build...).
        link.from().name() != "ctor" && link.from().name() != "datatest-derive"
    });
    let cargo_set = cargo_set_with_visitor(JsonFixture::metadata1(), "testcrate", &mut visitor);

    // No `ctor` not `datatest-derive` dependencies (unlike in `test_package_link_visitor_visits`).
    let package_names = cargo_set_package_names(&cargo_set)
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!package_names.contains("proc-macro2"));
    assert!(!package_names.contains("quote"));
    assert!(!package_names.contains("syn"));
    assert!(!package_names.contains("unicode-xid"));

    // No `syn` => ... links (unlike in `test_package_link_visitor_visits`).
    // No `quote` => ... links (unlike in `test_package_link_visitor_visits`).
    // No `proc-macro2` ... => links (unlike in `test_package_link_visitor_visits`).
    let trace = visitor.trace.into_iter().collect::<HashSet<_>>();
    assert!(!trace.contains("syn@1.0.5 => unicode-xid@0.2.0"));
    assert!(!trace.contains("syn@1.0.5 => quote@1.0.2"));
    assert!(!trace.contains("syn@1.0.5 => proc-macro2@1.0.3"));
    assert!(!trace.contains("quote@1.0.2 => proc-macro2@1.0.3"));
    assert!(!trace.contains("proc-macro2@1.0.3 => unicode-xid@0.2.0"));

    // The visitor was asked about these links, but didn't accept them.
    // Therefore these links should be present in the `trace`, but missing from
    // the final `cargo_set`.
    let cargo_set_links = links_to_strings(cargo_set.host_links())
        .into_iter()
        .collect::<HashSet<_>>();
    assert!(!cargo_set_links.contains("ctor@0.1.10 => syn@1.0.5"));
    assert!(!cargo_set_links.contains("ctor@0.1.10 => quote@1.0.2"));
    assert!(!cargo_set_links.contains("datatest-derive@0.4.0 => syn@1.0.5"));
    assert!(!cargo_set_links.contains("datatest-derive@0.4.0 => quote@1.0.2"));
    assert!(!cargo_set_links.contains("datatest-derive@0.4.0 => proc-macro2@1.0.3"));
    assert!(trace.contains("ctor@0.1.10 => syn@1.0.5"));
    assert!(trace.contains("ctor@0.1.10 => quote@1.0.2"));
    assert!(trace.contains("datatest-derive@0.4.0 => syn@1.0.5"));
    assert!(trace.contains("datatest-derive@0.4.0 => quote@1.0.2"));
    assert!(trace.contains("datatest-derive@0.4.0 => proc-macro2@1.0.3"));
}
