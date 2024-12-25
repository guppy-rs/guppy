// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

mod custom;
mod expr;
mod helpers;

datatest_stable::harness! {
    { test = custom::custom_invalid, root = custom::CUSTOM_INVALID_PATH, pattern = r"^.*/*" },
    { test = expr::expr_invalid, root = expr::EXPR_INVALID_PATH, pattern = r"^.*/*" },
}
