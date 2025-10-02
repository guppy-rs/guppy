// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use anyhow::Result;
use clap::Parser;
use fixture_manager::FixtureManager;

fn main() -> Result<()> {
    let args = FixtureManager::parse();
    args.exec()
}
