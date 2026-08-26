// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Add and remove dependencies.

use crate::{
    HakariBuilder, WorkspaceHackLineStyle,
    cli_ops::{WorkspaceOp, WorkspaceOps},
    hakari::DepFormatVersion,
};
use guppy::{
    VersionReq,
    graph::{DependencyDirection, PackageLink, PackageMetadata, PackageSet},
};

impl<'g> HakariBuilder<'g> {
    /// Returns the set of operations that need to be performed to add the workspace-hack
    /// dependency to the given set of workspace crates.
    ///
    /// Also includes remove operations for the workspace-hack dependency from excluded crates.
    ///
    /// Returns `None` if the hakari package wasn't specified at construction time.
    ///
    /// Requires the `cli-support` feature to be enabled.
    pub fn manage_dep_ops(&self, workspace_set: &PackageSet<'g>) -> Option<WorkspaceOps<'g, '_>> {
        let graph = self.graph();
        let hakari_package = self.hakari_package()?;

        let (add_to, remove_from) =
            workspace_set.filter_partition(DependencyDirection::Reverse, |package| {
                // manage-deps only rewrites manifests inside the workspace.
                // Ignore anything else.
                if !package.in_workspace() {
                    return None;
                }
                let link_opt = package
                    .link_to(hakari_package.id())
                    .expect("valid package ID");
                let should_be_included = self.is_managed_member(&package);
                match (link_opt, should_be_included) {
                    (None, true) => Some(true),
                    (Some(_), false) => Some(false),
                    (Some(link), true) => match self.dep_format_version {
                        DepFormatVersion::V1 => None,
                        DepFormatVersion::V2 | DepFormatVersion::V3 | DepFormatVersion::V4 => {
                            needs_update_v2(hakari_package, link, self.workspace_hack_line_style)
                                .then_some(true)
                        }
                    },
                    (None, false) => None,
                }
            });

        let mut ops = Vec::with_capacity(2);
        if !add_to.is_empty() {
            ops.push(WorkspaceOp::AddDependency {
                name: hakari_package.name(),
                crate_path: hakari_package
                    .source()
                    .workspace_path()
                    .expect("hakari package is in workspace"),
                version: hakari_package.version(),
                dep_format: self.dep_format_version,
                line_style: self.workspace_hack_line_style,
                add_to,
            });
        }
        if !remove_from.is_empty() {
            ops.push(WorkspaceOp::RemoveDependency {
                name: hakari_package.name(),
                remove_from,
            });
        }
        Some(WorkspaceOps::new(graph, ops))
    }

    /// Returns the set of operations that need to be performed to add the workspace-hack
    /// dependency to the given set of workspace crates.
    ///
    /// Returns `None` if the hakari package wasn't specified at construction time.
    ///
    /// Requires the `cli-support` feature to be enabled.
    pub fn add_dep_ops(
        &self,
        workspace_set: &PackageSet<'g>,
        force: bool,
    ) -> Option<WorkspaceOps<'g, '_>> {
        let graph = self.graph();
        let hakari_package = self.hakari_package()?;

        let add_to = if force {
            workspace_set.clone()
        } else {
            workspace_set.filter(DependencyDirection::Reverse, |package| {
                let link_opt = package
                    .link_to(hakari_package.id())
                    .expect("valid package ID");
                match link_opt {
                    Some(link) => {
                        needs_update_v2(hakari_package, link, self.workspace_hack_line_style)
                    }
                    None => true,
                }
            })
        };

        let op = if !add_to.is_empty() {
            Some(WorkspaceOp::AddDependency {
                name: hakari_package.name(),
                version: hakari_package.version(),
                crate_path: hakari_package
                    .source()
                    .workspace_path()
                    .expect("hakari package is in workspace"),
                dep_format: self.dep_format_version,
                line_style: self.workspace_hack_line_style,
                add_to,
            })
        } else {
            None
        };
        Some(WorkspaceOps::new(graph, op))
    }

    /// Returns the set of operations that need to be performed to remove the workspace-hack
    /// dependency from the given set of workspace crates.
    ///
    /// Returns `None` if the hakari package wasn't specified at construction time.
    ///
    /// Requires the `cli-support` feature to be enabled.
    pub fn remove_dep_ops(
        &self,
        workspace_set: &PackageSet<'g>,
        force: bool,
    ) -> Option<WorkspaceOps<'g, '_>> {
        let graph = self.graph();
        let hakari_package = self.hakari_package()?;

        let remove_from = if force {
            workspace_set.clone()
        } else {
            workspace_set.filter(DependencyDirection::Reverse, |package| {
                graph
                    .directly_depends_on(package.id(), hakari_package.id())
                    .expect("valid package ID")
            })
        };

        let op = if !remove_from.is_empty() {
            Some(WorkspaceOp::RemoveDependency {
                name: hakari_package.name(),
                remove_from,
            })
        } else {
            None
        };
        Some(WorkspaceOps::new(graph, op))
    }
}

#[allow(clippy::if_same_then_else, clippy::needless_bool)]
fn needs_update_v2(
    hakari_package: &PackageMetadata<'_>,
    link: PackageLink<'_>,
    line_style: WorkspaceHackLineStyle,
) -> bool {
    if !link.version_req().matches(hakari_package.version()) {
        // The version number doesn't match: it must be updated.
        true
    } else if link.version_req() == &VersionReq::STAR {
        // The version number isn't specified. Require it in case line_style isn't workspace-dotted.
        match line_style {
            WorkspaceHackLineStyle::Full | WorkspaceHackLineStyle::VersionOnly => true,
            WorkspaceHackLineStyle::WorkspaceDotted => false,
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_ops::WorkspaceOp;
    use fixtures::{
        json::{
            JsonFixture, METADATA_HAKARI_REVERSE_DEP_HACK_DEP,
            METADATA_HAKARI_REVERSE_DEP_MEMBER_B, METADATA_HAKARI_REVERSE_DEP_MEMBER_D,
        },
        package_id,
    };
    use std::collections::BTreeSet;

    #[test]
    fn manage_dep_ops_skips_non_workspace_packages() {
        let fixture = JsonFixture::metadata_hakari_reverse_dep();
        let graph = fixture.graph();
        let hakari_id = fixture
            .details()
            .hakari_package()
            .expect("hakari-reverse-dep fixture names a hakari package");
        let mut builder =
            HakariBuilder::new(graph, Some(hakari_id)).expect("hakari builder is created");
        // V1 doesn't rewrite existing dependency lines, so set the format
        // version to v4 to also exercise the "already depends on the hack, but
        // needs an update" path.
        builder.set_dep_format_version(DepFormatVersion::V4);

        // * hrd-member-d is a workspace member without a dependency on the hakari package.
        // * hrd-member-b is a workspace member that depends on the hakari package with `req = "*"`.
        // * hrd-hack-dep is a non-workspace package that depends on the hakari package.
        let member_d_id = package_id(METADATA_HAKARI_REVERSE_DEP_MEMBER_D);
        let member_b_id = package_id(METADATA_HAKARI_REVERSE_DEP_MEMBER_B);
        let hack_dep_id = package_id(METADATA_HAKARI_REVERSE_DEP_HACK_DEP);
        let package_set = graph
            .resolve_ids([&member_d_id, &member_b_id, &hack_dep_id])
            .expect("all package IDs are known to the graph");

        let ops = builder
            .manage_dep_ops(&package_set)
            .expect("hakari package was specified, so ops are returned");
        let mut add_to = None;
        for op in ops.ops() {
            match op {
                WorkspaceOp::AddDependency { add_to: set, .. } => {
                    assert!(add_to.is_none(), "at most one add op is generated");
                    add_to = Some(set);
                }
                WorkspaceOp::RemoveDependency { remove_from, .. } => {
                    let remove_ids: Vec<_> = remove_from
                        .package_ids(DependencyDirection::Forward)
                        .collect();
                    panic!(
                        "hrd-hack-dep is outside the workspace and the other two \
                         packages are managed members, so nothing in the set \
                         should have the hack removed, but got a remove op for \
                         {remove_ids:?}"
                    );
                }
                WorkspaceOp::NewCrate { .. } => {
                    panic!("manage-deps never creates crates");
                }
            }
        }

        let add_to = add_to.expect("an add op is generated");
        let add_ids: BTreeSet<_> = add_to.package_ids(DependencyDirection::Forward).collect();
        let expected_ids: BTreeSet<_> = [&member_d_id, &member_b_id].into_iter().collect();
        assert_eq!(
            add_ids, expected_ids,
            "hrd-member-d has no dependency on the hack and hrd-member-b's \
             `req = \"*\"` needs updating under dep format V4, so both are \
             added to; hrd-hack-dep is outside the workspace, so it is ignored"
        );
    }
}
