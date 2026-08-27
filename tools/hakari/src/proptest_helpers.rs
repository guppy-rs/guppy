// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    HakariBuilder, UnifyTargetHost,
    hakari::{DepFormatVersion, WorkspaceHackLineStyle},
};
use guppy::{
    PackageId,
    graph::{PackageGraph, cargo::CargoResolverVersion},
    platform::{Platform, TargetFeatures},
};
use proptest::{
    collection::{hash_set, vec},
    prelude::*,
};

/// ## Helpers for property testing
///
/// The methods in this section allow random instances of a `HakariBuilder` to be generated, for use
/// in property-based testing scenarios.
///
/// Requires the `proptest1` feature to be enabled.
impl<'g> HakariBuilder<'g> {
    /// Returns a `Strategy` that generates random `HakariBuilder` instances based on this graph.
    ///
    /// Requires the `proptest1` feature to be enabled.
    ///
    /// ## Panics
    ///
    /// Panics if:
    /// * there are no packages in this `PackageGraph`, or
    /// * `hakari_id` is specified but it isn't known to the graph, or isn't in the workspace.
    pub fn proptest1_strategy(
        graph: &'g PackageGraph,
        hakari_id_strategy: impl Strategy<Value = Option<&'g PackageId>> + 'g,
    ) -> impl Strategy<Value = HakariBuilder<'g>> + 'g {
        (
            hakari_id_strategy,
            vec(Platform::strategy(any::<TargetFeatures>()), 0..4),
            any::<CargoResolverVersion>(),
            hash_set(graph.proptest1_id_strategy(), 0..8),
            hash_set(graph.proptest1_id_strategy(), 0..8),
            any::<UnifyTargetHost>(),
            any::<bool>(),
            any::<DepFormatVersion>(),
            any::<WorkspaceHackLineStyle>(),
        )
            .prop_map(
                move |(
                    hakari_id,
                    platforms,
                    version,
                    traversal_excludes,
                    final_excludes,
                    unify_target_host,
                    output_single_feature,
                    dep_format_version,
                    line_style,
                )| {
                    let mut builder = HakariBuilder::new(graph, hakari_id)
                        .expect("HakariBuilder::new returned an error");
                    let platforms: Vec<_> = platforms
                        .iter()
                        .map(|platform| platform.triple_str().to_owned())
                        .collect();
                    builder
                        .set_platforms(platforms)
                        .expect("all platforms are known")
                        .set_resolver(version)
                        .add_traversal_excludes(traversal_excludes)
                        .expect("traversal excludes obtained from PackageGraph should work")
                        .add_final_excludes(final_excludes)
                        .expect("final excludes obtained from PackageGraph should work")
                        .set_unify_target_host(unify_target_host)
                        .set_dep_format_version(dep_format_version)
                        .set_workspace_hack_line_style(line_style)
                        .set_output_single_feature(output_single_feature);
                    builder
                },
            )
    }
}

#[cfg(all(test, feature = "cli-support"))]
mod test {
    use super::*;
    use fixtures::json::JsonFixture;
    use guppy::graph::{DependencyDirection, PackageMetadata};
    use proptest::option;
    use std::collections::HashSet;

    /// Ensure that HakariBuilder roundtrips to its summary format.
    #[test]
    fn builder_summary_roundtrip() {
        for (&name, fixture) in JsonFixture::all_fixtures() {
            let graph = fixture.graph();
            let workspace = graph.workspace();
            let strategy = HakariBuilder::proptest1_strategy(
                graph,
                option::of(workspace.proptest1_id_strategy()),
            );
            proptest!(|(builder in strategy)| {
                let summary = builder.to_summary().unwrap_or_else(|err| {
                    panic!("for fixture {name}, builder -> summary conversion failed: {err}");
                });
                let builder2 = summary.to_hakari_builder(graph).unwrap_or_else(|err| {
                    panic!("for fixture {name}, summary -> builder conversion failed: {err}");
                });
                let summary2 = builder2.to_summary().unwrap_or_else(|err| {
                    panic!("for fixture {name}, second builder -> summary conversion failed: {err}");
                });
                assert_eq!(summary, summary2, "summary roundtripped correctly");
            });
        }
    }

    /// Ensure that HakariBuilder's traversal_excludes and is_traversal_excluded match up.
    #[test]
    fn traversal_excludes() {
        for (&name, fixture) in JsonFixture::all_fixtures() {
            let graph = fixture.graph();
            let workspace = graph.workspace();
            let strategy = HakariBuilder::proptest1_strategy(
                graph,
                option::of(workspace.proptest1_id_strategy()),
            );
            proptest!(|(builder in strategy, queries in vec(graph.proptest1_id_strategy(), 0..64))| {
                // Ensure that the hakari package is omitted.
                if let Some(package) = builder.hakari_package() {
                    assert!(
                        builder.is_traversal_excluded(package.id()).expect("valid package ID"),
                        "for fixture {name}, hakari package is excluded from traversals",
                    );
                }
                // Ensure that omits_package and omitted_packages match.
                let traversal_excludes: HashSet<_> = builder.traversal_excludes().collect();
                for query_id in queries {
                    assert_eq!(
                        traversal_excludes.contains(query_id),
                        builder.is_traversal_excluded(query_id).expect("valid package ID"),
                        "for fixture {name}, traversal_excludes and is_traversal_excluded match",
                    );
                }
            });
        }
    }

    /// Ensure that the structural excludes are exactly the third-party
    /// packages that reach the hakari package or a managed member.
    #[test]
    fn structural_excludes() {
        for (&name, fixture) in JsonFixture::all_fixtures() {
            let graph = fixture.graph();
            let workspace = graph.workspace();
            let strategy = HakariBuilder::proptest1_strategy(
                graph,
                option::of(workspace.proptest1_id_strategy()),
            );
            proptest!(|(builder in strategy)| {
                // This checks the reverse query itself rather than going
                // through compute(), since the latter is quite slow especially
                // in debug mode.
                let structural_excludes = builder.make_structural_excludes().cycle_forming;

                let is_root = |package: &PackageMetadata<'_>| {
                    builder
                        .hakari_package()
                        .is_some_and(|hakari_package| hakari_package.id() == package.id())
                        || builder.is_managed_member(package)
                };

                let reaches_root = |package: &PackageMetadata<'_>| {
                    package
                        .to_package_query(DependencyDirection::Forward)
                        .resolve_with_fn(|_, link| !link.dev_only())
                        .packages(DependencyDirection::Forward)
                        .any(|dep| is_root(&dep))
                };

                // Soundness -- every ID in the set is a third-party package
                // that really does reach a root.
                for package_id in &structural_excludes {
                    let package = graph.metadata(package_id).expect("valid package ID");
                    assert!(
                        !package.in_workspace(),
                        "for fixture {name}, structurally excluded {} is a third-party package",
                        package.name(),
                    );
                    assert!(
                        reaches_root(&package),
                        "for fixture {name}, structurally excluded {} reaches the hakari package \
                         or a managed member",
                        package.name(),
                    );
                }

                // Completeness -- non-dev links are acyclic, so it is enough to
                // check that no third-party package outside the set has a
                // non-dev link to a root or to a member of the set. Workspace
                // packages are never in the set, so links into the workspace
                // are checked with a forward query instead; dedupe them since
                // that query is the expensive part.
                let mut workspace_targets = HashSet::new();
                for package in graph.packages() {
                    if package.in_workspace() || structural_excludes.contains(package.id()) {
                        continue;
                    }
                    for link in package.direct_links() {
                        if link.dev_only() {
                            continue;
                        }
                        let to = link.to();
                        assert!(
                            !is_root(&to),
                            "for fixture {name}, {} isn't structurally excluded, so it has no \
                             non-dev link to root {}",
                            package.name(),
                            to.name(),
                        );
                        assert!(
                            !structural_excludes.contains(to.id()),
                            "for fixture {name}, {} isn't structurally excluded, so it has no \
                             non-dev link to structurally excluded {}",
                            package.name(),
                            to.name(),
                        );
                        if to.in_workspace() {
                            workspace_targets.insert(to.id());
                        }
                    }
                }
                for package_id in workspace_targets {
                    let package = graph.metadata(package_id).expect("valid package ID");
                    assert!(
                        !reaches_root(&package),
                        "for fixture {name}, workspace package {} is reached by a \
                         third-party package that isn't structurally excluded, so it must not \
                         reach a root itself",
                        package.name(),
                    );
                }
            });
        }
    }
}
