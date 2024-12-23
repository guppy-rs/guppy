// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

mod custom;
mod expr;
mod helpers;

datatest_stable::harness!(
    // Custom JSON
    custom::custom_invalid,
    custom::CUSTOM_INVALID_PATH,
    r"^.*/*",
    // Invalid expressions
    expr::expr_invalid,
    expr::EXPR_INVALID_PATH,
    r"^.*/*",
);
