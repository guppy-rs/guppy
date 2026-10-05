// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    errors::{ExpectedError, Result},
    helpers::read_config_contents,
};
use camino::Utf8Path;
use guppy::graph::{PackageGraph, PackageMetadata, PackageSet};
use hakari::{
    CargoTomlError, Hakari, HakariBuilder, HakariCargoToml, HakariOutputOptions,
    cli_ops::WorkspaceOps,
    summaries::{DEFAULT_CONFIG_PATH, FALLBACK_CONFIG_PATH, HakariBuilderSummary, HakariConfig},
    verify::VerifyErrors,
};

#[derive(Debug)]
pub(crate) struct BuilderWithHakariPackage<'g> {
    // `hakari_package` must be the same as `builder.hakari_package()`.
    builder: HakariBuilder<'g>,
    hakari_package: PackageMetadata<'g>,
}

impl<'g> BuilderWithHakariPackage<'g> {
    fn from_summary(
        graph: &'g PackageGraph,
        summary: &HakariBuilderSummary,
        config_path: &Utf8Path,
    ) -> Result<Self> {
        let builder = summary.to_hakari_builder(graph).map_err(|error| {
            ExpectedError::ConfigResolveFailed {
                config_path: config_path.to_owned(),
                error: Box::new(error),
            }
        })?;
        let Some(&hakari_package) = builder.hakari_package() else {
            return Err(ExpectedError::HakariPackageNotSet {
                config_path: config_path.to_owned(),
            });
        };
        Ok(Self {
            builder,
            hakari_package,
        })
    }

    pub(crate) fn graph(&self) -> &'g PackageGraph {
        self.builder.graph()
    }

    pub(crate) fn hakari_package(&self) -> PackageMetadata<'g> {
        self.hakari_package
    }

    pub(crate) fn compute(&self) -> Hakari<'g> {
        self.builder.clone().compute()
    }

    pub(crate) fn verify(self) -> Result<(), VerifyErrors<'g>> {
        self.builder.verify()
    }

    pub(crate) fn read_toml(&self) -> Result<HakariCargoToml, CargoTomlError> {
        self.builder
            .read_toml()
            .expect("builder has a hakari package, checked at construction")
    }

    pub(crate) fn manage_dep_ops(&self, workspace_set: &PackageSet<'g>) -> WorkspaceOps<'g, '_> {
        self.builder
            .manage_dep_ops(workspace_set)
            .expect("builder has a hakari package, checked at construction")
    }

    pub(crate) fn add_dep_ops(
        &self,
        workspace_set: &PackageSet<'g>,
        force: bool,
    ) -> WorkspaceOps<'g, '_> {
        self.builder
            .add_dep_ops(workspace_set, force)
            .expect("builder has a hakari package, checked at construction")
    }

    pub(crate) fn remove_dep_ops(
        &self,
        workspace_set: &PackageSet<'g>,
        force: bool,
    ) -> WorkspaceOps<'g, '_> {
        self.builder
            .remove_dep_ops(workspace_set, force)
            .expect("builder has a hakari package, checked at construction")
    }
}

pub(crate) fn make_builder_and_output(
    package_graph: &PackageGraph,
) -> Result<(BuilderWithHakariPackage<'_>, HakariOutputOptions)> {
    let (config_path, contents) = read_config_contents(
        package_graph.workspace().root(),
        [DEFAULT_CONFIG_PATH, FALLBACK_CONFIG_PATH],
    )?;

    builder_and_output_from_config(package_graph, &config_path, &contents)
}

pub(crate) fn builder_and_output_from_config<'g>(
    package_graph: &'g PackageGraph,
    config_path: &Utf8Path,
    contents: &str,
) -> Result<(BuilderWithHakariPackage<'g>, HakariOutputOptions)> {
    let config: HakariConfig =
        contents
            .parse()
            .map_err(|error| ExpectedError::ConfigDeserializeFailed {
                config_path: config_path.to_owned(),
                error,
            })?;

    let builder =
        BuilderWithHakariPackage::from_summary(package_graph, &config.builder, config_path)?;
    let hakari_output = config.output.to_options();

    Ok((builder, hakari_output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::reverse_dep_builder;
    use fixtures::json::{JsonFixture, METADATA_HAKARI_REVERSE_DEP_WORKSPACE_HACK};
    use guppy::PackageId;

    const CONFIG_PATH: &str = "/workspace/custom/hakari.toml";

    fn resolve_reverse_dep_config(
        contents: &str,
    ) -> Result<(BuilderWithHakariPackage<'static>, HakariOutputOptions)> {
        builder_and_output_from_config(
            JsonFixture::metadata_hakari_reverse_dep().graph(),
            Utf8Path::new(CONFIG_PATH),
            contents,
        )
    }

    #[test]
    fn config_without_hakari_package_is_rejected() {
        let error = resolve_reverse_dep_config("resolver = \"2\"\n")
            .expect_err("config without hakari-package is rejected");
        let ExpectedError::HakariPackageNotSet { config_path } = &error else {
            panic!("expected HakariPackageNotSet, found {error:?}");
        };
        assert_eq!(config_path, Utf8Path::new(CONFIG_PATH));
    }

    #[test]
    fn config_with_unknown_hakari_package_is_rejected() {
        let error = resolve_reverse_dep_config("hakari-package = \"nope\"\nresolver = \"2\"\n")
            .expect_err("config with unknown hakari-package is rejected");
        let ExpectedError::ConfigResolveFailed {
            config_path,
            error: guppy_error,
        } = &error
        else {
            panic!("expected ConfigResolveFailed, found {error:?}");
        };
        assert_eq!(config_path, Utf8Path::new(CONFIG_PATH));
        let guppy::Error::UnknownWorkspaceName(name) = guppy_error.as_ref() else {
            panic!("expected UnknownWorkspaceName, found {guppy_error:?}");
        };
        assert_eq!(name, "nope");
    }

    #[test]
    fn config_that_does_not_deserialize_is_rejected() {
        for contents in [
            // Not valid TOML.
            "resolver = \n",
            // Valid TOML, but resolver has the wrong type.
            "resolver = 2\n",
            // Valid TOML, but the required resolver is missing.
            "",
        ] {
            let error = resolve_reverse_dep_config(contents)
                .expect_err("config that doesn't deserialize is rejected");
            let ExpectedError::ConfigDeserializeFailed {
                config_path,
                error: _,
            } = &error
            else {
                panic!("expected ConfigDeserializeFailed for {contents:?}, found {error:?}");
            };
            assert_eq!(config_path, Utf8Path::new(CONFIG_PATH));
        }
    }

    #[test]
    fn config_with_hakari_package_resolves() {
        let (builder, _) = resolve_reverse_dep_config(
            "hakari-package = \"hrd-workspace-hack\"\nresolver = \"2\"\n",
        )
        .expect("config with hakari-package resolves");
        assert_eq!(
            builder.hakari_package().id(),
            &PackageId::new(METADATA_HAKARI_REVERSE_DEP_WORKSPACE_HACK),
        );
    }

    #[test]
    fn hakari_package_accessors_do_not_panic() {
        let builder = reverse_dep_builder();
        let workspace = builder.graph().resolve_workspace();

        builder.manage_dep_ops(&workspace);
        builder.add_dep_ops(&workspace, false);
        builder.remove_dep_ops(&workspace, false);
        // The result of `read_toml` doesn't matter because the path
        // (`/Users/fakeuser etc) is likely not on disk (though it might be!)
        let _ = builder.read_toml();
    }
}
