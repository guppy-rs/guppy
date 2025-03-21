// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Implementations for options shared by commands.

use clap::{ArgEnum, Parser};
use color_eyre::eyre::{Result, WrapErr, ensure, eyre};
use guppy::{
    PackageId,
    graph::{DependencyDirection, DependencyReq, PackageGraph, PackageLink, PackageQuery},
    platform::EnabledTernary,
};
use guppy_cmdlib::string_to_platform_spec;
use std::collections::HashSet;

#[derive(ArgEnum, Copy, Clone, Debug)]
pub enum Kind {
    All,
    Workspace,
    DirectThirdParty,
    ThirdParty,
}

impl Kind {
    /// Returns true if this link should be traversed.
    pub fn should_traverse(self, link: &PackageLink<'_>) -> bool {
        // NOTE: We always retain all workspace deps in the graph, otherwise
        // we'll get a disconnected graph.
        match self {
            Kind::All | Kind::ThirdParty => true,
            Kind::DirectThirdParty => link.from().in_workspace(),
            Kind::Workspace => link.from().in_workspace() && link.to().in_workspace(),
        }
    }
}

#[derive(Debug, Parser)]
pub struct QueryOptions {
    /// Query reverse transitive dependencies (default: forward)
    #[clap(long = "query-reverse", parse(from_flag = parse_direction))]
    direction: DependencyDirection,

    #[clap(rename_all = "screaming_snake_case")]
    /// The root packages to start the query from
    roots: Vec<String>,
}

impl QueryOptions {
    /// Constructs a `PackageQuery` based on these options.
    pub fn apply<'g>(&self, pkg_graph: &'g PackageGraph) -> Result<PackageQuery<'g>> {
        if !self.roots.is_empty() {
            // NOTE: The root set packages are specified by name. The tool currently
            // does not handle multiple version of the same package as the current use
            // cases are passing workspace members as the root set, which won't be
            // duplicated.
            let root_set = self.roots.iter().map(|s| s.as_str()).collect();
            Ok(pkg_graph.query_directed(names_to_ids(pkg_graph, root_set), self.direction)?)
        } else {
            ensure!(
                self.direction == DependencyDirection::Forward,
                eyre!("--query-reverse requires roots to be specified")
            );
            Ok(pkg_graph.query_workspace())
        }
    }
}

#[derive(Debug, Parser)]
pub struct BaseFilterOptions {
    #[clap(long, rename_all = "kebab-case", name = "package")]
    /// Omit edges that point into a given package; useful for seeing how
    /// removing a dependency affects the graph
    pub omit_edges_into: Vec<String>,

    #[clap(long, short, arg_enum, default_value = "all")]
    /// Kind of crates to select
    pub kind: Kind,
}

impl BaseFilterOptions {
    /// Return the set of omitted package IDs.
    pub fn omitted_package_ids<'g: 'a, 'a>(
        &'a self,
        pkg_graph: &'g PackageGraph,
    ) -> impl Iterator<Item = &'g PackageId> + 'a {
        let omitted_set: HashSet<&str> = self.omit_edges_into.iter().map(|s| s.as_str()).collect();
        names_to_ids(pkg_graph, omitted_set)
    }
}

#[derive(Debug, Parser)]
pub struct FilterOptions {
    #[clap(flatten)]
    pub base_opts: BaseFilterOptions,

    #[clap(long, rename_all = "kebab-case")]
    /// Include dev dependencies
    pub include_dev: bool,

    #[clap(long, rename_all = "kebab-case")]
    /// Include build dependencies
    pub include_build: bool,

    #[clap(long)]
    /// Target to filter, "current", "any" or "always" [default: any]
    pub target: Option<String>,
}

impl FilterOptions {
    /// Construct a package resolver based on the filter options.
    pub fn make_resolver<'g>(
        &'g self,
        pkg_graph: &'g PackageGraph,
    ) -> Result<impl Fn(&PackageQuery<'g>, PackageLink<'g>) -> bool + 'g> {
        let omitted_package_ids: HashSet<_> =
            self.base_opts.omitted_package_ids(pkg_graph).collect();

        let platform_spec = string_to_platform_spec(self.target.as_deref())
            .wrap_err_with(|| "target platform isn't known")?;

        let ret = move |_: &PackageQuery<'g>, link| {
            // filter by the kind of dependency (--kind)
            let include_kind = self.base_opts.kind.should_traverse(&link);

            let include_type = self.eval(link, |req| {
                req.status().enabled_on(&platform_spec.clone()) != EnabledTernary::Disabled
            });

            // filter out provided edge targets (--omit-edges-into)
            let include_edge = !omitted_package_ids.contains(link.to().id());

            include_kind && include_type && include_edge
        };
        Ok(ret)
    }

    /// Select normal, dev, or build dependencies as requested (--include-build, --include-dev), and
    /// apply `pred_fn` to whatever's selected.
    fn eval(
        &self,
        link: PackageLink<'_>,
        mut pred_fn: impl FnMut(DependencyReq<'_>) -> bool,
    ) -> bool {
        pred_fn(link.normal())
            || self.include_dev && pred_fn(link.dev())
            || self.include_build && pred_fn(link.build())
    }
}

pub(crate) fn parse_direction(reverse: bool) -> DependencyDirection {
    if reverse {
        DependencyDirection::Reverse
    } else {
        DependencyDirection::Forward
    }
}

pub(crate) fn names_to_ids<'g: 'a, 'a>(
    pkg_graph: &'g PackageGraph,
    names: HashSet<&'a str>,
) -> impl Iterator<Item = &'g PackageId> + 'a {
    pkg_graph.packages().filter_map(move |metadata| {
        if names.contains(metadata.name()) {
            Some(metadata.id())
        } else {
            None
        }
    })
}
