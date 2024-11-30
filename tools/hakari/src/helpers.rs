// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use guppy::Version;
use std::fmt;

/// A formatting wrapper that may print out a minimum version that would match the provided version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct VersionDisplay<'a> {
    version: &'a Version,
    exact_versions: bool,
    // Adding build metadata is incorrect (Cargo emits a warning when build metadata is present in a
    // dependency specification), but support this for compatibility with older versions of hakari.
    with_build_metadata: bool,
}

impl<'a> VersionDisplay<'a> {
    pub(crate) fn new(
        version: &'a Version,
        exact_versions: bool,
        with_build_metadata: bool,
    ) -> Self {
        Self {
            version,
            exact_versions,
            with_build_metadata,
        }
    }
}

impl fmt::Display for VersionDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.exact_versions && self.version.pre.is_empty() {
            // Minimal versions permitted, so attempt to minimize the version.
            if self.version.major >= 1 {
                return write!(f, "{}", self.version.major);
            }
            if self.version.minor >= 1 {
                return write!(f, "{}.{}", self.version.major, self.version.minor);
            }
        }
        write!(
            f,
            "{}.{}.{}",
            self.version.major, self.version.minor, self.version.patch
        )?;
        if !self.version.pre.is_empty() {
            write!(f, "-{}", self.version.pre)?;
        }
        if self.with_build_metadata && !self.version.build.is_empty() {
            write!(f, "+{}", self.version.build)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fixtures::json::*;
    use guppy::{graph::DependencyDirection, VersionReq};

    #[test]
    fn min_version() {
        let versions = vec![
            ("1.4.0", "1", "1.4.0", "1", "1.4.0"),
            ("2.8.0", "2", "2.8.0", "2", "2.8.0"),
            ("0.4.2", "0.4", "0.4.2", "0.4", "0.4.2"),
            ("0.0.7", "0.0.7", "0.0.7", "0.0.7", "0.0.7"),
            ("1.4.0-b1", "1.4.0-b1", "1.4.0-b1", "1.4.0-b1", "1.4.0-b1"),
            (
                "2.8.0-a.1+v123",
                "2.8.0-a.1",
                "2.8.0-a.1",
                "2.8.0-a.1+v123",
                "2.8.0-a.1+v123",
            ),
            ("4.2.3+g456", "4", "4.2.3", "4", "4.2.3+g456"),
        ];

        for (version_str, min, exact, min_with_build_metadata, exact_with_build_metadata) in
            versions
        {
            let version = Version::parse(version_str).expect("valid version");
            let version_req = VersionReq::parse(min).expect("valid version req");
            assert!(
                version_req.matches(&version),
                "version req {} should match version {}",
                min,
                version
            );
            assert_eq!(
                &format!("{}", VersionDisplay::new(&version, false, false)),
                min
            );
            assert_eq!(
                &format!("{}", VersionDisplay::new(&version, true, false)),
                exact
            );
            assert_eq!(
                &format!("{}", VersionDisplay::new(&version, false, true)),
                min_with_build_metadata
            );
            assert_eq!(
                &format!("{}", VersionDisplay::new(&version, true, true)),
                exact_with_build_metadata
            );
        }
    }

    #[test]
    fn min_versions_match() {
        for (&name, fixture) in JsonFixture::all_fixtures() {
            let graph = fixture.graph();
            for package in graph.resolve_all().packages(DependencyDirection::Forward) {
                let version = package.version();
                let min_version = format!("{}", VersionDisplay::new(version, false, false));
                let version_req = VersionReq::parse(&min_version).expect("valid version req");

                assert!(
                    version_req.matches(version),
                    "for fixture '{}', for package '{}', min version req {} should match version {}",
                    name,
                    package.id(),
                    min_version,
                    version,
                );

                let min_version_with_build_metadata =
                    format!("{}", VersionDisplay::new(version, false, true));
                let version_req =
                    VersionReq::parse(&min_version_with_build_metadata).expect("valid version req");

                assert!(
                    version_req.matches(version),
                    "for fixture '{}', for package '{}', min version (with build metadata) req {} should match version {}",
                    name,
                    package.id(),
                    min_version,
                    version,
                );
            }
        }
    }
}
