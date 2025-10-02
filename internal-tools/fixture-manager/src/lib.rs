// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod context;
pub mod hakari_toml;
pub mod summaries;

use crate::{
    context::{ContextImpl, GenerateContext},
    hakari_toml::HakariTomlContext,
    summaries::*,
};
use anyhow::{Result, anyhow, bail};
use clap::{Parser, ValueEnum};
use fixtures::json::JsonFixture;

#[derive(Debug, Parser)]
pub struct FixtureManager {
    // TODO: add global options
    #[clap(subcommand)]
    cmd: Command,
}

impl FixtureManager {
    pub fn exec(self) -> Result<()> {
        match self.cmd {
            Command::List => list(),
            Command::GenerateSummaries(opts) => opts.exec(),
            Command::GenerateHakari(opts) => opts.exec(),
        }
    }
}

#[derive(Debug, Parser)]
enum Command {
    #[clap(name = "list")]
    /// List fixtures
    List,
    /// Generate summaries
    GenerateSummaries(GenerateSummariesOpts),
    /// Generate Hakari outputs
    GenerateHakari(GenerateHakariOpts),
}

pub fn list() -> Result<()> {
    for (name, fixture) in JsonFixture::all_fixtures().iter() {
        println!("{}: {}", name, fixture.workspace_path());
    }

    Ok(())
}

#[derive(Debug, Parser)]
pub struct GenerateSummariesOpts {
    /// Number of summaries to generate
    #[clap(long, default_value = Self::DEFAULT_COUNT_STR)]
    pub count: usize,

    #[clap(flatten)]
    pub generate_opts: GenerateOpts,
}

impl GenerateSummariesOpts {
    /// The default value of the `count` field, as a string.
    pub const DEFAULT_COUNT_STR: &'static str = "8";

    /// The default value of the `count` field.
    pub fn default_count() -> usize {
        Self::DEFAULT_COUNT_STR
            .parse()
            .expect("DEFAULT_COUNT_STR should parse as a usize")
    }
}

#[derive(Debug, Parser)]
pub struct GenerateHakariOpts {
    /// Number of options to generate
    #[clap(long, default_value = Self::DEFAULT_COUNT_STR)]
    pub count: usize,

    #[clap(flatten)]
    pub generate_opts: GenerateOpts,
}

impl GenerateHakariOpts {
    /// The default value of the `count` field, as a string.
    pub const DEFAULT_COUNT_STR: &'static str = "4";

    /// The default value of the `count` field.
    pub fn default_count() -> usize {
        Self::DEFAULT_COUNT_STR
            .parse()
            .expect("DEFAULT_COUNT_STR should parse as a usize")
    }
}

#[derive(Debug, Parser)]
pub struct GenerateOpts {
    /// Execution mode (check, force or generate)
    #[clap(
        long,
        short,
        value_enum,
        ignore_case = true,
        default_value = "generate"
    )]
    pub mode: GenerateMode,

    /// Only generate outputs for these fixtures
    #[clap(long)]
    pub fixtures: Vec<String>,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum GenerateMode {
    Generate,
    Check,
    Force,
}

impl GenerateSummariesOpts {
    pub fn exec(self) -> Result<()> {
        self.generate_opts.exec::<SummaryContext>(self.count)
    }
}

impl GenerateHakariOpts {
    pub fn exec(self) -> Result<()> {
        self.generate_opts.exec::<HakariTomlContext>(self.count)
    }
}

impl GenerateOpts {
    pub fn exec<'g, T: ContextImpl<'g>>(self, args: T::IterArgs) -> Result<()> {
        let fixtures: Box<dyn Iterator<Item = (&str, &JsonFixture)>> = if self.fixtures.is_empty() {
            Box::new(
                JsonFixture::all_fixtures()
                    .iter()
                    .map(|(name, fixture)| (*name, fixture)),
            )
        } else {
            let fixtures = self
                .fixtures
                .iter()
                .map(|name| {
                    let fixture = JsonFixture::by_name(name)
                        .ok_or_else(|| anyhow!("unknown fixture: {}", name))?;
                    Ok((name.as_str(), fixture))
                })
                .collect::<Result<Vec<_>>>()?;
            Box::new(fixtures.into_iter())
        };

        let mut num_changed = 0;

        for (name, fixture) in fixtures {
            println!("generating outputs for {name}...");

            let context: GenerateContext<'_, T> =
                GenerateContext::new(fixture, &args, self.mode == GenerateMode::Force)?;
            for item in context {
                let item = item?;
                let is_changed = item.is_changed();

                if is_changed {
                    num_changed += 1;
                }

                if self.mode == GenerateMode::Check {
                    if is_changed {
                        println!("** {}:\n{}", item.path(), item.diff());
                    }

                    continue;
                }

                if is_changed || self.mode == GenerateMode::Force {
                    item.write_to_path()?;
                }
            }
        }

        if self.mode == GenerateMode::Check && num_changed > 0 {
            bail!("{} outputs changed", num_changed);
        }

        println!("{num_changed} outputs changed");

        Ok(())
    }
}
