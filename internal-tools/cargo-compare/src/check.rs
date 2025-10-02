// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    GlobalContext,
    common::GuppyCargoCommon,
    diff::{FeatureDiff, TargetHostDiff},
};
use clap::Parser;
use color_eyre::eyre::{Result, eyre};
use guppy::graph::cargo::CargoResolverVersion;
use guppy_cmdlib::CargoMetadataOptions;
use proptest::{
    prelude::*,
    test_runner::{Config, TestError, TestRunner},
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Options for cargo/guppy comparisons.
#[derive(Debug, Parser)]
pub struct CheckOpts {
    /// Number of randomly generated diff operations to run
    #[clap(long, default_value = "256")]
    pub cases: u32,
    /// Print a message every n test cases. Use '0' to disable
    #[clap(long, default_value = "16")]
    pub print_every: usize,
    #[clap(flatten)]
    pub metadata: CargoMetadataOptions,
    /// Print out unchanged packages and features as well
    #[clap(long)]
    pub verbose: bool,
    // TODO: add resolver to cargo metadata
    /// Use v2 resolver (must match resolver in workspace Cargo.toml)
    #[clap(long)]
    pub v2_resolver: bool,
}

impl CheckOpts {
    /// Executes this command.
    pub fn exec(self, ctx: &GlobalContext) -> Result<()> {
        let resolver = if self.v2_resolver {
            CargoResolverVersion::V2
        } else {
            CargoResolverVersion::V1
        };
        let strat = GuppyCargoCommon::strategy(&self.metadata, ctx.graph, resolver);

        let mut testrunner = TestRunner::new(Config {
            cases: self.cases,
            ..Config::default()
        });

        // print a message after every n tests
        let test_count = AtomicUsize::new(0);

        testrunner
            .run(&strat, |common| {
                let cargo_map = common
                    .resolve_cargo(ctx)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                let guppy_map = common
                    .resolve_guppy(ctx)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;

                let target_diff = FeatureDiff {
                    graph: ctx.graph(),
                    a: guppy_map.target_map,
                    b: cargo_map.target_map,
                    verbose: self.verbose,
                };

                let host_diff = FeatureDiff {
                    graph: ctx.graph(),
                    a: guppy_map.host_map,
                    b: cargo_map.host_map,
                    verbose: self.verbose,
                };

                let diff = TargetHostDiff::new(target_diff, host_diff);

                if self.print_every != 0 {
                    let test_count = test_count.fetch_add(1, Ordering::SeqCst);
                    if test_count.is_multiple_of(self.print_every) && test_count != 0 {
                        println!("finished running {test_count} tests");
                    };
                };

                prop_assert!(!diff.any_diff(), "unexpected diff: {}", diff);

                Ok(())
            })
            .map_err(|e| match e {
                TestError::Abort(e) => {
                    eyre!("Aborted cargo/guppy diff check, {:?}", e)
                }
                TestError::Fail(e, v) => {
                    eyre!("Failed cargo/guppy diff check {:?}\n{:?}", e, v)
                }
            })
    }
}
