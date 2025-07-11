// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    GlobalContext,
    common::{GuppyCargoCommon, anyhow_to_eyre},
};
use clap::Parser;
use color_eyre::eyre::{Result, bail};
use diffus::{Diffable, edit};
use guppy::{PackageId, graph::PackageGraph};
use itertools::Itertools;
use once_cell::sync::OnceCell;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Options for cargo/guppy comparisons.
#[derive(Debug, Parser)]
pub struct DiffOpts {
    #[clap(flatten)]
    pub common: GuppyCargoCommon,
    /// Print out unchanged packages and features as well
    #[clap(long)]
    pub verbose: bool,
}

impl DiffOpts {
    /// Executes this command.
    pub fn exec(self, ctx: &GlobalContext) -> Result<()> {
        let target_host_diff = self.compute_diff(ctx)?;
        println!("{target_host_diff}");

        if target_host_diff.any_diff() {
            bail!("non-empty diff!")
        } else {
            Ok(())
        }
    }

    pub fn compute_diff<'g>(self, ctx: &'g GlobalContext) -> Result<TargetHostDiff<'g>> {
        let cargo_map = anyhow_to_eyre(self.common.resolve_cargo(ctx))?;
        let guppy_map = self.common.resolve_guppy(ctx)?;

        // println!("guppy_map: {:#?}", guppy_map.host_map);
        // println!("cargo_map: {:#?}", cargo_map.host_map);

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

        Ok(TargetHostDiff::new(target_diff, host_diff))
    }
}

pub struct TargetHostDiff<'g> {
    pub target_diff: FeatureDiff<'g>,
    pub host_diff: FeatureDiff<'g>,
    any_diff: OnceCell<bool>,
}

impl<'g> TargetHostDiff<'g> {
    pub fn new(target_diff: FeatureDiff<'g>, host_diff: FeatureDiff<'g>) -> Self {
        Self {
            target_diff,
            host_diff,
            any_diff: OnceCell::new(),
        }
    }

    /// Returns true if there's a diff.
    pub fn any_diff(&self) -> bool {
        *self
            .any_diff
            .get_or_init(|| self.target_diff.any_diff() || self.host_diff.any_diff())
    }
}

impl fmt::Display for TargetHostDiff<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "** target diff (guppy -> cargo):\n{}\n",
            self.target_diff
        )?;
        write!(f, "** host diff (guppy -> cargo):\n{}\n", self.host_diff)
    }
}

pub struct FeatureDiff<'g> {
    pub graph: &'g PackageGraph,
    pub a: BTreeMap<PackageId, BTreeSet<String>>,
    pub b: BTreeMap<PackageId, BTreeSet<String>>,
    pub verbose: bool,
}

impl FeatureDiff<'_> {
    /// Returns true if there's a diff.
    pub fn any_diff(&self) -> bool {
        self.a.diff(&self.b).is_change()
    }
}

impl fmt::Display for FeatureDiff<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.a.diff(&self.b) {
            edit::Edit::Change(diff) => {
                for (pkg_id, diff) in diff {
                    use diffus::edit::map::Edit;

                    let package = self.graph.metadata(pkg_id).expect("valid package ID");
                    match diff {
                        Edit::Copy(features) => {
                            if self.verbose {
                                writeln!(
                                    f,
                                    "{} {}: unchanged\n  * features: {}",
                                    package.name(),
                                    package.version(),
                                    features.iter().join(", ")
                                )?
                            }
                        }
                        Edit::Insert(features) => writeln!(
                            f,
                            "{} {}: added\n  * new features: {}",
                            package.name(),
                            package.version(),
                            features.iter().join(", ")
                        )?,
                        Edit::Remove(features) => writeln!(
                            f,
                            "{} {}: removed\n  * old features: {}",
                            package.name(),
                            package.version(),
                            features.iter().join(", "),
                        )?,
                        Edit::Change(diff) => {
                            writeln!(
                                f,
                                "{} {}: changed, features:",
                                package.name(),
                                package.version(),
                            )?;
                            for (feature_name, diff) in diff {
                                use diffus::edit::set::Edit;

                                match diff {
                                    Edit::Copy(_) => {
                                        if self.verbose {
                                            writeln!(f, "  * {feature_name}: unchanged")?
                                        }
                                    }
                                    Edit::Insert(_) => {
                                        writeln!(f, "  * {feature_name}: added")?;
                                    }
                                    Edit::Remove(_) => {
                                        writeln!(f, "  * {feature_name}: removed")?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            edit::Edit::Copy(map) => {
                if self.verbose {
                    for (pkg_id, features) in map {
                        let package = self.graph.metadata(pkg_id).expect("valid package ID");

                        writeln!(
                            f,
                            "{} {}: unchanged\n  * features: {}",
                            package.name(),
                            package.version(),
                            features.iter().join(", ")
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}
