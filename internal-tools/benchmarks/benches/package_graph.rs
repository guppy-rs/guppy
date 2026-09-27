// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use guppy::{
    PackageId,
    graph::{
        DependencyDirection, PackageGraph, PackageMetadata, PackageSet,
        feature::{FeatureGraph, FeatureId, FeatureSet},
    },
};
use proptest::{collection::vec, prelude::*, sample::select};
use proptest_ext::ValueGenerator;
use std::{collections::HashMap, hint::black_box, time::Instant};

pub fn construct_benchmarks(c: &mut Criterion) {
    c.bench_function("make_package_graph", |b| b.iter(make_package_graph));
}

pub fn query_benchmarks(c: &mut Criterion) {
    let mut package_graph = make_package_graph();
    let mut cache = package_graph.new_depends_cache();
    let mut gen = ValueGenerator::deterministic();

    c.bench_function("depends_on", |b| {
        b.iter_batched_ref(
            || gen.generate(id_pairs_strategy(&package_graph)),
            |package_ids| {
                package_ids.iter().for_each(|(package_a, package_b)| {
                    let _ = package_graph.depends_on(package_a, package_b);
                })
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("depends_on_cache", |b| {
        b.iter_batched_ref(
            || gen.generate(id_pairs_strategy(&package_graph)),
            |package_ids| {
                package_ids.iter().for_each(|(package_a, package_b)| {
                    let _ = cache.depends_on(package_a, package_b);
                })
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("into_ids", |b| {
        b.iter_batched_ref(
            || gen.generate(ids_directions_strategy(&package_graph)),
            |ids_directions| {
                ids_directions
                    .iter()
                    .for_each(|(package_ids, query_direction, iter_direction)| {
                        let query = package_graph
                            .query_directed(package_ids.iter().copied(), *query_direction)
                            .unwrap();
                        let _: Vec<_> = query.resolve().package_ids(*iter_direction).collect();
                    })
            },
            BatchSize::SmallInput,
        )
    });

    c.bench_function("resolve_package_name", |b| {
        b.iter_custom(|iters| {
            package_graph.invalidate_caches();
            let start = Instant::now();
            for _ in 0..iters {
                let package_set = package_graph.resolve_package_name("syn");
                assert_eq!(package_set.len(), 2, "2 versions of syn");
            }
            start.elapsed()
        })
    });

    c.bench_function("make_package_name_hashmap", |b| {
        b.iter_with_large_drop(|| {
            let hashmap = make_package_name_hashmap(&package_graph);
            assert_eq!(
                hashmap.get("syn").map(|v| v.len()),
                Some(2),
                "2 versions of syn"
            );
        })
    });

    c.bench_function("make_cycles", |b| {
        b.iter(|| {
            package_graph.invalidate_caches();
            black_box(package_graph.cycles());
        })
    });
}

/// Benchmarks for iterating over sets: roots, topological order and links.
///
/// Each benchmark runs an operation over a fixed batch of sets, in both
/// directions. The batches are:
///
/// * `query`: sets returned by queries, which contain either all or none of
///   each dependency cycle.
/// * `split`: sets containing all but one member of the graph's largest
///   cycle, plus a few other nodes. These split a cycle.
/// * `small`: sets with a single member. Costs that scale with the size of the
///   graph rather than the set stand out here.
pub fn set_benchmarks(c: &mut Criterion) {
    let package_graph = make_package_graph();
    let mut gen = ValueGenerator::deterministic();

    let package_sets = [
        ("query", package_query_sets(&package_graph, &mut gen)),
        ("split", package_split_sets(&package_graph, &mut gen)),
        ("small", package_small_sets(&package_graph, &mut gen)),
    ];
    let mut group = c.benchmark_group("package_set");
    for (name, sets) in &package_sets {
        group.bench_function(format!("{name}/root_ids"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.root_ids(direction).count());
                })
            })
        });
        group.bench_function(format!("{name}/package_ids"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.package_ids(direction).count());
                })
            })
        });
        group.bench_function(format!("{name}/links"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.links(direction).count());
                })
            })
        });
    }
    group.finish();

    let feature_graph = package_graph.feature_graph();
    let feature_sets = [
        ("query", feature_query_sets(feature_graph, &mut gen)),
        ("split", feature_split_sets(feature_graph, &mut gen)),
        ("small", feature_small_sets(feature_graph, &mut gen)),
    ];
    let mut group = c.benchmark_group("feature_set");
    for (name, sets) in &feature_sets {
        group.bench_function(format!("{name}/root_ids"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.root_ids(direction).count());
                })
            })
        });
        group.bench_function(format!("{name}/feature_ids"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.feature_ids(direction).count());
                })
            })
        });
        group.bench_function(format!("{name}/links"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.links(direction).count());
                })
            })
        });
        group.bench_function(format!("{name}/packages_with_features"), |b| {
            b.iter(|| {
                for_each_direction(sets, |set, direction| {
                    black_box(set.packages_with_features(direction).count());
                })
            })
        });
    }
    group.finish();
}

/// The number of sets in each batch.
const SET_BATCH_LEN: usize = 16;

fn for_each_direction<S>(sets: &[S], mut f: impl FnMut(&S, DependencyDirection)) {
    for set in sets {
        for direction in [DependencyDirection::Forward, DependencyDirection::Reverse] {
            f(set, direction);
        }
    }
}

fn package_query_sets<'g>(
    graph: &'g PackageGraph,
    gen: &mut ValueGenerator,
) -> Vec<PackageSet<'g>> {
    gen.generate(ids_directions_strategy(graph))
        .into_iter()
        .map(|(package_ids, query_direction, _)| {
            graph
                .query_directed(package_ids, query_direction)
                .expect("valid package IDs")
                .resolve()
        })
        .collect()
}

fn package_split_sets<'g>(
    graph: &'g PackageGraph,
    gen: &mut ValueGenerator,
) -> Vec<PackageSet<'g>> {
    let cycle = graph
        .cycles()
        .all_cycles()
        .max_by_key(|cycle| cycle.len())
        .expect("benchmark graph has a cycle");
    assert!(cycle.len() >= 3, "largest cycle can be split");
    let strategy = vec(
        (
            vec(graph.proptest1_id_strategy(), 8),
            select((0..cycle.len()).collect::<Vec<_>>()),
        ),
        SET_BATCH_LEN,
    );
    gen.generate(strategy)
        .into_iter()
        .map(|(other_ids, skip)| {
            let cycle_ids = cycle
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, id)| *id);
            graph
                .resolve_ids(other_ids.into_iter().chain(cycle_ids))
                .expect("valid package IDs")
        })
        .collect()
}

fn package_small_sets<'g>(
    graph: &'g PackageGraph,
    gen: &mut ValueGenerator,
) -> Vec<PackageSet<'g>> {
    gen.generate(vec(graph.proptest1_id_strategy(), SET_BATCH_LEN))
        .into_iter()
        .map(|package_id| graph.resolve_ids([package_id]).expect("valid package ID"))
        .collect()
}

fn feature_query_sets<'g>(
    graph: FeatureGraph<'g>,
    gen: &mut ValueGenerator,
) -> Vec<FeatureSet<'g>> {
    let strategy = vec(
        (
            vec(graph.proptest1_id_strategy(), 32),
            any::<DependencyDirection>(),
        ),
        SET_BATCH_LEN,
    );
    gen.generate(strategy)
        .into_iter()
        .map(|(feature_ids, query_direction)| {
            graph
                .query_directed(feature_ids, query_direction)
                .expect("valid feature IDs")
                .resolve()
        })
        .collect()
}

fn feature_split_sets<'g>(
    graph: FeatureGraph<'g>,
    gen: &mut ValueGenerator,
) -> Vec<FeatureSet<'g>> {
    let cycle: Vec<FeatureId<'g>> = graph
        .cycles()
        .all_cycles()
        .max_by_key(|cycle| cycle.len())
        .expect("benchmark graph has a feature cycle");
    assert!(cycle.len() >= 3, "largest feature cycle can be split");
    let strategy = vec(
        (
            vec(graph.proptest1_id_strategy(), 8),
            select((0..cycle.len()).collect::<Vec<_>>()),
        ),
        SET_BATCH_LEN,
    );
    gen.generate(strategy)
        .into_iter()
        .map(|(other_ids, skip)| {
            let cycle_ids = cycle
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, id)| *id);
            graph
                .resolve_ids(other_ids.into_iter().chain(cycle_ids))
                .expect("valid feature IDs")
        })
        .collect()
}

fn feature_small_sets<'g>(
    graph: FeatureGraph<'g>,
    gen: &mut ValueGenerator,
) -> Vec<FeatureSet<'g>> {
    gen.generate(vec(graph.proptest1_id_strategy(), SET_BATCH_LEN))
        .into_iter()
        .map(|feature_id| graph.resolve_ids([feature_id]).expect("valid feature ID"))
        .collect()
}

fn make_package_graph() -> PackageGraph {
    // Use this package graph as a large and representative one.
    PackageGraph::from_json(include_str!(
        "../../../fixtures/large/metadata_libra_9ffd93b.json"
    ))
    .unwrap()
}

fn make_package_name_hashmap<'g>(
    graph: &'g PackageGraph,
) -> HashMap<&'g str, Vec<PackageMetadata<'g>>> {
    // Testing the real HashMap is fine here.
    #[allow(clippy::disallowed_methods)]
    let mut hashmap: HashMap<&'g str, Vec<_>> = HashMap::new();
    for package in graph.packages() {
        hashmap.entry(package.name()).or_default().push(package);
    }

    hashmap
}

/// Generate pairs of IDs for benchmarks.
fn id_pairs_strategy(graph: &PackageGraph) -> impl Strategy<Value = Vec<(&PackageId, &PackageId)>> {
    vec(
        (graph.proptest1_id_strategy(), graph.proptest1_id_strategy()),
        256,
    )
}

/// Generate IDs and directions for benchmarks.
fn ids_directions_strategy(
    graph: &PackageGraph,
) -> impl Strategy<Value = Vec<(Vec<&PackageId>, DependencyDirection, DependencyDirection)>> {
    vec(
        (
            vec(graph.proptest1_id_strategy(), 32),
            any::<DependencyDirection>(),
            any::<DependencyDirection>(),
        ),
        16,
    )
}

criterion_group!(
    benches,
    construct_benchmarks,
    query_benchmarks,
    set_benchmarks
);
criterion_main!(benches);
