// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![cfg(feature = "serde1")]

use fixtures::json::JsonFixture;
use guppy::graph::PackageGraph;

#[test]
fn serde_roundtrip_json() {
    let graph = JsonFixture::metadata1().graph();

    let json = serde_json::to_string(&graph).expect("serialization failed");
    let graph2: PackageGraph = serde_json::from_str(&json).expect("deserialization failed");

    // Verify internal invariants on the deserialized graph.
    graph2.verify().expect("verification failed");

    // Basic structural checks, since `PackageGraph` doesn't implement `PartialEq`.
    assert_eq!(
        graph.package_count(),
        graph2.package_count(),
        "package count mismatch"
    );
    assert_eq!(
        graph.link_count(),
        graph2.link_count(),
        "link count mismatch"
    );
    assert_eq!(
        graph.workspace().root(),
        graph2.workspace().root(),
        "workspace root mismatch"
    );
}
