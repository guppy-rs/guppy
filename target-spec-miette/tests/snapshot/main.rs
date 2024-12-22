// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

// TODO: add snapshot tests for the other errors as well.

mod custom;

datatest_stable::harness!(
    custom::test_invalid_custom_json,
    "tests/snapshot/custom-invalid/input",
    r"^.*/*",
);
