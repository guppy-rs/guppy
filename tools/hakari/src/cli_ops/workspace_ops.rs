// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{
    hakari::{DepFormatVersion, WorkspaceHackLineStyle},
    helpers::VersionDisplay,
};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use guppy::{
    Version,
    graph::{DependencyDirection, PackageGraph, PackageMetadata, PackageSet},
};
use owo_colors::{OwoColorize, Style};
use std::{borrow::Cow, cmp::Ordering, collections::BTreeMap, error, fmt, fs, io, io::Write};
use thiserror::Error;
use toml_edit::{
    Array, DocumentMut, Formatted, InlineTable, Item, Table, TableLike, TomlError, Value,
};

/// Represents a set of write operations to the workspace.
#[derive(Clone, Debug)]
pub struct WorkspaceOps<'g, 'a> {
    graph: &'g PackageGraph,
    ops: Vec<WorkspaceOp<'g, 'a>>,
}

impl<'g, 'a> WorkspaceOps<'g, 'a> {
    pub(crate) fn new(
        graph: &'g PackageGraph,
        ops: impl IntoIterator<Item = WorkspaceOp<'g, 'a>>,
    ) -> Self {
        Self {
            graph,
            ops: ops.into_iter().collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn ops(&self) -> &[WorkspaceOp<'g, 'a>] {
        &self.ops
    }

    /// Returns a displayer for the workspace operations.
    #[inline]
    pub fn display<'ops>(&'ops self) -> WorkspaceOpsDisplay<'g, 'a, 'ops> {
        WorkspaceOpsDisplay::new(self)
    }

    /// Returns true if no workspace operations are specified.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Apply these workspace operations.
    ///
    /// Returns an error if any operations failed to complete.
    pub fn apply(&self) -> Result<(), ApplyError> {
        let workspace_root = self.graph.workspace().root();
        let canonical_workspace_root = workspace_root.canonicalize_utf8().map_err(|error| {
            ApplyError::new(
                workspace_root,
                ApplyErrorKind::CanonicalizeWorkspaceRoot { error },
            )
        })?;
        for op in &self.ops {
            op.apply(&canonical_workspace_root)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) enum WorkspaceOp<'g, 'a> {
    NewCrate {
        crate_path: &'a Utf8Path,
        files: BTreeMap<Cow<'a, Utf8Path>, Cow<'a, [u8]>>,
        root_files: BTreeMap<Cow<'a, Utf8Path>, Cow<'a, [u8]>>,
    },
    AddDependency {
        name: &'a str,
        crate_path: &'a Utf8Path,
        version: &'a Version,
        dep_format: DepFormatVersion,
        line_style: WorkspaceHackLineStyle,
        add_to: PackageSet<'g>,
    },
    RemoveDependency {
        name: &'a str,
        remove_from: PackageSet<'g>,
    },
}

impl<'g> WorkspaceOp<'g, '_> {
    fn apply(&self, canonical_workspace_root: &Utf8Path) -> Result<(), ApplyError> {
        match self {
            WorkspaceOp::NewCrate {
                crate_path,
                files,
                root_files,
            } => {
                Self::create_new_crate(canonical_workspace_root, crate_path, files)?;
                // Now that the crate has been created, we can canonicalize it.
                let crate_path = canonical_rel_path(crate_path, canonical_workspace_root)?;

                for (rel_path, contents) in root_files {
                    let abs_path = canonical_workspace_root.join(rel_path.as_ref());
                    let parent = abs_path.parent().expect("abs path should have a parent");
                    std::fs::create_dir_all(parent).map_err(|error| {
                        ApplyError::new(parent, ApplyErrorKind::CreateDirs { error })
                    })?;
                    write_contents(contents, &abs_path)?;
                }

                Self::add_to_root_toml(canonical_workspace_root, &crate_path)
            }
            WorkspaceOp::AddDependency {
                name,
                crate_path,
                version,
                dep_format,
                line_style,
                add_to,
            } => {
                let crate_path = canonical_rel_path(crate_path, canonical_workspace_root)?;
                for package in add_to.packages(DependencyDirection::Reverse) {
                    Self::add_to_cargo_toml(
                        name,
                        version,
                        &crate_path,
                        *dep_format,
                        *line_style,
                        package,
                    )?;
                }
                Ok(())
            }
            WorkspaceOp::RemoveDependency { name, remove_from } => {
                for package in remove_from.packages(DependencyDirection::Reverse) {
                    Self::remove_from_cargo_toml(name, package)?;
                }
                Ok(())
            }
        }
    }

    // ---
    // Helper methods
    // ---

    fn create_new_crate(
        workspace_root: &Utf8Path,
        crate_path: &Utf8Path,
        files: &BTreeMap<Cow<'_, Utf8Path>, Cow<'_, [u8]>>,
    ) -> Result<(), ApplyError> {
        let abs_path = workspace_root.join(crate_path);
        for (path, contents) in files {
            // Create parent directories if necessary.
            let mut dir_path = match path.parent() {
                Some(parent) => abs_path.join(parent),
                None => abs_path.clone(),
            };
            std::fs::create_dir_all(&dir_path).map_err(|error| {
                ApplyError::new(&dir_path, ApplyErrorKind::CreateDirs { error })
            })?;

            // Write out the file.
            dir_path.push(
                path.file_name().ok_or_else(|| {
                    ApplyError::new(path.as_ref(), ApplyErrorKind::MissingFileName)
                })?,
            );
            write_contents(contents, &dir_path)?;
        }
        Ok(())
    }

    fn add_to_root_toml(
        workspace_root: &Utf8Path,
        crate_path: &Utf8Path,
    ) -> Result<(), ApplyError> {
        let root_toml_path = workspace_root.join("Cargo.toml");

        let mut doc = read_toml(&root_toml_path)?;
        Self::add_to_workspace_members(&mut doc, crate_path).map_err(|error| {
            ApplyError::new(&root_toml_path, ApplyErrorKind::WorkspaceSection { error })
        })?;
        write_document(&doc, &root_toml_path)
    }

    fn add_to_workspace_members(
        doc: &mut DocumentMut,
        crate_path: &Utf8Path,
    ) -> Result<(), WorkspaceSectionError> {
        let members = Self::get_workspace_members_array(doc)?;

        let add = |members: &mut Array, idx: usize| {
            // idx can be within the array (0..members.len()) or at the end (members.len() + 1).
            let existing = if idx < members.len() {
                members.get(idx).expect("valid idx")
            } else {
                members.get(members.len() - 1).expect("valid idx")
            };

            let write_path = with_forward_slashes(crate_path).into_string();
            let write_path = decorate(existing, write_path);
            members.insert_formatted(idx, write_path);
        };

        let mut written = false;
        for idx in 0..members.len() {
            let member = members.get(idx).expect("valid idx");
            match member.as_str() {
                Some(path) => {
                    let path = Utf8Path::new(path);
                    // Insert the crate path before the first element greater than it. If the list
                    // is kept in alphabetical order, this works out correctly.
                    match path.cmp(crate_path) {
                        Ordering::Greater => {
                            add(members, idx);
                            written = true;
                            break;
                        }
                        Ordering::Equal => {
                            // The crate path already exists -- skip it.
                            written = true;
                            break;
                        }
                        Ordering::Less => {}
                    }
                }
                None => {
                    return Err(WorkspaceSectionError::MemberNotAString {
                        index: idx,
                        found: member.type_name(),
                    });
                }
            }
        }

        if !written {
            add(members, members.len());
        }

        Ok(())
    }

    fn get_workspace_members_array(
        doc: &mut DocumentMut,
    ) -> Result<&mut Array, WorkspaceSectionError> {
        let doc_table = doc.as_table_mut();
        let workspace_table = match doc_table.get_mut("workspace") {
            Some(Item::Table(workspace_table)) => workspace_table,
            Some(other) => {
                return Err(WorkspaceSectionError::NotATable {
                    found: other.type_name(),
                });
            }
            None => {
                return Err(WorkspaceSectionError::NotFound);
            }
        };

        let members = match workspace_table.get_mut("members") {
            Some(Item::Value(Value::Array(members))) => members,
            Some(other) => {
                return Err(WorkspaceSectionError::MembersNotAnArray {
                    found: other.type_name(),
                });
            }
            None => {
                return Err(WorkspaceSectionError::MembersNotFound);
            }
        };
        Ok(members)
    }

    fn add_to_cargo_toml(
        name: &str,
        version: &Version,
        crate_path: &Utf8Path,
        dep_format: DepFormatVersion,
        line_style: WorkspaceHackLineStyle,
        package: PackageMetadata<'g>,
    ) -> Result<(), ApplyError> {
        let manifest_path = package.manifest_path();
        let mut doc = read_toml(manifest_path)?;

        let package_path = package
            .source()
            .workspace_path()
            .expect("package should be in workspace");
        // Find the location of the new path (relative) with respect to the package path.
        let path = pathdiff::diff_utf8_paths(crate_path, package_path)
            .expect("both new_path and package_path are relative");

        let path_table = Self::inline_table_for_add(version, dep_format, line_style, &path);

        add_dependency_to_document(&mut doc, name, path_table)
            .map_err(|error| ApplyError::new(manifest_path, ApplyErrorKind::NotATable { error }))?;

        write_document(&doc, manifest_path)
    }

    fn inline_table_for_add(
        version: &Version,
        dep_format: DepFormatVersion,
        line_style: WorkspaceHackLineStyle,
        path: &Utf8Path,
    ) -> InlineTable {
        let mut itable = InlineTable::new();

        match line_style {
            WorkspaceHackLineStyle::Full => {
                // Pass in exact_versions = false because we don't want unnecessary churn in the unlikely
                // event that a published workspace-hack version has a minor bump in it.
                let version_str = format!(
                    "{}",
                    VersionDisplay::new(version, false, dep_format < DepFormatVersion::V3)
                );
                if dep_format >= DepFormatVersion::V2 {
                    itable.insert("version", version_str.into());
                }

                let mut path = Formatted::new(with_forward_slashes(path).into_string());
                if dep_format == DepFormatVersion::V1 {
                    // Previous versions of `cargo hakari` accidentally missed adding the space to the end
                    // of the line. Newer versions of toml_edit do that automatically, so restore the old
                    // behavior.
                    path.decor_mut().set_suffix("");
                }
                itable.insert("path", Value::String(path));

                if dep_format == DepFormatVersion::V2 {
                    itable.fmt();
                }
                itable
            }
            WorkspaceHackLineStyle::VersionOnly => {
                // Pass in exact_versions = false because we don't want unnecessary churn in the unlikely
                // event that a published workspace-hack version has a minor bump in it.
                let version_str = format!("{}", VersionDisplay::new(version, false, false));
                itable.insert("version", version_str.into());
                itable
            }
            WorkspaceHackLineStyle::WorkspaceDotted => {
                // Pass in exact_versions = false because we don't want unnecessary churn in the unlikely
                // event that a published workspace-hack version has a minor bump in it.
                itable.insert("workspace", true.into());
                itable.set_dotted(true);
                itable
            }
        }
    }

    fn remove_from_cargo_toml(name: &str, package: PackageMetadata<'g>) -> Result<(), ApplyError> {
        let manifest_path = package.manifest_path();
        let mut doc = read_toml(manifest_path)?;
        remove_dependency_from_document(&mut doc, name)
            .map_err(|error| ApplyError::new(manifest_path, ApplyErrorKind::NotATable { error }))?;

        write_document(&doc, manifest_path)
    }
}

// ---
// Manifest edits
// ---

/// The name of a `Cargo.toml` dependency section.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencySection {
    /// A normal dependency in the `[dependencies]` or `[target.*.dependencies]`
    /// sections.
    Normal,
    /// A dev dependency in the `[dev-dependencies]` or
    /// `[target.*.dev-dependencies]` sections.
    Dev,
    /// A build dependency in the `[build-dependencies]` or
    /// `[target.*.build-dependencies]` sections.
    Build,
}

impl DependencySection {
    const ALL: [Self; 3] = [Self::Normal, Self::Dev, Self::Build];

    fn key(self) -> &'static str {
        match self {
            DependencySection::Normal => "dependencies",
            DependencySection::Dev => "dev-dependencies",
            DependencySection::Build => "build-dependencies",
        }
    }
}

/// An error while editing a manifest document: an entry that should be a
/// table isn't one.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[non_exhaustive]
pub enum NotATableError {
    /// A top-level dependency section.
    #[error("[{}] is not a table", .section.key())]
    Section {
        /// The section that is not a table.
        section: DependencySection,
    },
    /// The `[target]` table.
    #[error("[target] is not a table")]
    Target,
    /// A `[target.<platform>]` entry.
    #[error("[target.'{platform}'] is not a table")]
    Platform {
        /// The platform key, without TOML quoting or escapes.
        platform: String,
    },
    /// A `[target.<platform>.<section>]` entry.
    #[error("[target.'{platform}'.{}] is not a table", .section.key())]
    PlatformSection {
        /// The platform key, without TOML quoting or escapes.
        platform: String,
        /// The section under the platform that is not a table.
        section: DependencySection,
    },
}

/// Adds or replaces the entry for `name` in `[dependencies]`, creating the
/// section if needed.
///
/// Also drops `name` from `[dev-dependencies]`. A dev dependency is a strict
/// subset of a normal one.
///
/// `[build-dependencies]` is left alone since it has meaningfully different
/// semantics. (We may want to also remove them in the future, though.)
fn add_dependency_to_document(
    doc: &mut DocumentMut,
    name: &str,
    dep: InlineTable,
) -> Result<(), NotATableError> {
    let dep_table = get_or_insert_dependency_section(doc, DependencySection::Normal)?;
    dep_table.insert(name, Item::Value(Value::InlineTable(dep)));
    if let Some(dev_table) = get_dependency_section(doc, DependencySection::Dev)? {
        dev_table.remove(name);
    }
    Ok(())
}

/// Removes the entry for `name` from every dependency section it appears in,
/// including the platform-specific sections under `[target]`.
fn remove_dependency_from_document(
    doc: &mut DocumentMut,
    name: &str,
) -> Result<(), NotATableError> {
    // TODO: someone might have added the workspace-hack package under a different name.
    // Handle that if someone complains.
    for section in DependencySection::ALL {
        if let Some(dep_table) = get_dependency_section(doc, section)? {
            dep_table.remove(name);
        }
    }

    let Some(target_item) = doc.as_table_mut().get_mut("target") else {
        return Ok(());
    };
    let Some(target_table) = target_item.as_table_like_mut() else {
        return Err(NotATableError::Target);
    };
    for (platform, platform_item) in target_table.iter_mut() {
        let platform = platform.get();
        let Some(platform_table) = platform_item.as_table_like_mut() else {
            return Err(NotATableError::Platform {
                platform: platform.to_owned(),
            });
        };
        for section in DependencySection::ALL {
            let Some(section_item) = platform_table.get_mut(section.key()) else {
                continue;
            };
            let Some(dep_table) = section_item.as_table_like_mut() else {
                return Err(NotATableError::PlatformSection {
                    platform: platform.to_owned(),
                    section,
                });
            };
            dep_table.remove(name);
        }
    }
    Ok(())
}

/// Returns the given dependency section, or `None` if it doesn't exist.
fn get_dependency_section(
    doc: &mut DocumentMut,
    section: DependencySection,
) -> Result<Option<&mut dyn TableLike>, NotATableError> {
    let key = section.key();
    match doc.as_table_mut().get_mut(key) {
        Some(item) => match item.as_table_like_mut() {
            Some(table) => Ok(Some(table)),
            None => Err(NotATableError::Section { section }),
        },
        None => Ok(None),
    }
}

fn get_or_insert_dependency_section(
    doc: &mut DocumentMut,
    section: DependencySection,
) -> Result<&mut dyn TableLike, NotATableError> {
    let key = section.key();
    let doc_table = doc.as_table_mut();

    if doc_table.contains_key(key) {
        match doc_table
            .get_mut(key)
            .expect("just checked for presence of section")
            .as_table_like_mut()
        {
            Some(table) => Ok(table),
            None => Err(NotATableError::Section { section }),
        }
    } else {
        // Add the table.
        let mut new_table = Table::new();
        new_table.set_implicit(true);
        doc_table.insert(key, Item::Table(new_table));
        let table = doc_table
            .get_mut(key)
            .expect("was just inserted")
            .as_table_like_mut()
            .expect("was just inserted");
        Ok(table)
    }
}

fn decorate(existing: &Value, new: impl Into<Value>) -> Value {
    let decor = existing.decor();
    new.into().decorated(
        decor.prefix().cloned().unwrap_or_default(),
        decor.suffix().cloned().unwrap_or_default(),
    )
}

// Always write out paths with forward slashes, including on Windows.
fn with_forward_slashes(path: &Utf8Path) -> Utf8PathBuf {
    let components: Vec<_> = path.iter().collect();
    components.join("/").into()
}

// ---
// Path functions
// ---

fn canonical_rel_path(
    path: &Utf8Path,
    canonical_base: &Utf8Path,
) -> Result<Utf8PathBuf, ApplyError> {
    let abs_path = canonical_base.join(path);
    // Canonicalize the path now to remove .. etc.
    let canonical_path = abs_path
        .canonicalize_utf8()
        .map_err(|error| ApplyError::new(&abs_path, ApplyErrorKind::CanonicalizePath { error }))?;
    match canonical_path.strip_prefix(canonical_base) {
        Ok(rel_path) => Ok(rel_path.to_owned()),
        Err(_) => {
            // This can happen under some symlink scenarios.
            Err(ApplyError::new(
                &abs_path,
                ApplyErrorKind::PathOutsideBase {
                    canonical_path,
                    canonical_base: canonical_base.to_owned(),
                },
            ))
        }
    }
}

// ---
// Read/write functions
// ---

fn read_toml(manifest_path: &Utf8Path) -> Result<DocumentMut, ApplyError> {
    let toml = fs::read_to_string(manifest_path)
        .map_err(|error| ApplyError::new(manifest_path, ApplyErrorKind::ReadManifest { error }))?;
    parse_toml(&toml, manifest_path)
}

fn parse_toml(toml: &str, manifest_path: &Utf8Path) -> Result<DocumentMut, ApplyError> {
    toml.parse::<DocumentMut>()
        .map_err(|error| ApplyError::new(manifest_path, ApplyErrorKind::ParseManifest { error }))
}

fn write_contents(contents: &[u8], path: &Utf8Path) -> Result<(), ApplyError> {
    write_atomic(path, |file| file.write_all(contents))
}

fn write_document(document: &DocumentMut, path: &Utf8Path) -> Result<(), ApplyError> {
    write_atomic(path, |file| write!(file, "{document}"))
}

fn write_atomic(
    path: &Utf8Path,
    cb: impl FnOnce(&mut fs::File) -> Result<(), io::Error>,
) -> Result<(), ApplyError> {
    let atomic_file = AtomicFile::new(path, OverwriteBehavior::AllowOverwrite);
    match atomic_file.write(cb) {
        Ok(()) => Ok(()),
        Err(atomicwrites::Error::Internal(error)) | Err(atomicwrites::Error::User(error)) => {
            Err(ApplyError::new(path, ApplyErrorKind::WriteFile { error }))
        }
    }
}

/// An error that occurred while writing out changes to a workspace.
#[derive(Debug)]
pub struct ApplyError {
    path: Utf8PathBuf,
    kind: Box<ApplyErrorKind>,
}

impl ApplyError {
    /// Returns the path at which the error occurred.
    #[inline]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Returns the kind of error that occurred.
    #[inline]
    pub fn kind(&self) -> &ApplyErrorKind {
        &self.kind
    }

    fn new(path: impl Into<Utf8PathBuf>, kind: ApplyErrorKind) -> Self {
        Self {
            path: path.into(),
            kind: Box::new(kind),
        }
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "for path {}, {}", self.path, self.kind)
    }
}

impl error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        // The kind's message is part of this error's Display, so skip over
        // the kind in the source chain to avoid printing its message twice.
        error::Error::source(&*self.kind)
    }
}

/// The kind of error that occurred while applying workspace operations.
///
/// Returned by [`ApplyError::kind`]. To access the corresponding path, use
/// [`ApplyError::path`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ApplyErrorKind {
    /// The workspace root could not be canonicalized.
    #[error("unable to canonicalize workspace root")]
    CanonicalizeWorkspaceRoot {
        /// The underlying error.
        #[source]
        error: io::Error,
    },

    /// A path, when joined to the canonical workspace root, could not be canonicalized.
    #[error("error canonicalizing path")]
    CanonicalizePath {
        /// The underlying error.
        #[source]
        error: io::Error,
    },

    /// The canonical form of the path is not within the canonical workspace root.
    ///
    /// This can happen due to symlinks, `..` components, or an absolute path.
    #[error("canonical path is not within base path {canonical_base}")]
    PathOutsideBase {
        /// The canonical form of the path.
        canonical_path: Utf8PathBuf,
        /// The canonical base the path was expected to be within.
        canonical_base: Utf8PathBuf,
    },

    /// The directory at the path, or one of its parents, could not be created.
    #[error("error creating directories")]
    CreateDirs {
        /// The underlying error.
        #[source]
        error: io::Error,
    },

    /// The path of a file to be created does not end in a file name.
    ///
    /// Here, [`ApplyError::path`] is relative to the new crate's directory.
    #[error("does not contain a file name")]
    MissingFileName,

    /// The manifest could not be read.
    #[error("error reading TOML file")]
    ReadManifest {
        /// The underlying error.
        #[source]
        error: io::Error,
    },

    /// The manifest is not valid TOML.
    #[error("error deserializing TOML file")]
    ParseManifest {
        /// The underlying error.
        #[source]
        error: TomlError,
    },

    /// The file could not be written.
    #[error("error writing file")]
    WriteFile {
        /// The underlying error.
        #[source]
        error: io::Error,
    },

    /// The root manifest's `[workspace]` section does not have the shape needed
    /// to add a member to it.
    #[error(transparent)]
    WorkspaceSection {
        /// What is wrong with the section.
        error: WorkspaceSectionError,
    },

    /// An entry in the manifest that should be a table is not one.
    #[error(transparent)]
    NotATable {
        /// Which entry is not a table.
        error: NotATableError,
    },
}

/// An error occurred modifying the root manifest's `[workspace]` section.
///
/// Part of [`ApplyErrorKind::WorkspaceSection`].
#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[non_exhaustive]
pub enum WorkspaceSectionError {
    /// The root manifest has no `[workspace]` section.
    #[error("[workspace] section not found")]
    NotFound,

    /// `[workspace]` in the root manifest is not a table.
    ///
    /// Currently, an inline table is rejected as well.
    #[error("expected [workspace] to be a table, found {found}")]
    NotATable {
        /// The name of the TOML type found instead, as returned by toml_edit's
        /// [`Item::type_name`].
        found: &'static str,
    },

    /// The root manifest does not have a `workspace.members` field.
    #[error("workspace.members not found")]
    MembersNotFound,

    /// `workspace.members` in the root manifest is not an array.
    #[error("expected workspace.members to be an array, found {found}")]
    MembersNotAnArray {
        /// The name of the TOML type found instead, as returned by toml_edit's
        /// [`Item::type_name`].
        found: &'static str,
    },

    /// An element of `workspace.members` in the root manifest is not a string.
    #[error("workspace.members contains non-strings")]
    MemberNotAString {
        /// zero-based index of the element.
        index: usize,
        /// The name of the TOML type found instead, as returned by toml_edit's
        /// [`Item::type_name`].
        found: &'static str,
    },
}

/// A display formatter for [`WorkspaceOps`].
#[derive(Clone, Debug)]
pub struct WorkspaceOpsDisplay<'g, 'a, 'ops> {
    ops: &'ops WorkspaceOps<'g, 'a>,
    styles: Box<Styles>,
}

impl<'g, 'a, 'ops> WorkspaceOpsDisplay<'g, 'a, 'ops> {
    fn new(ops: &'ops WorkspaceOps<'g, 'a>) -> Self {
        Self {
            ops,
            styles: Box::default(),
        }
    }

    /// Adds ANSI color codes to the output.
    pub fn colorize(&mut self) -> &mut Self {
        self.styles.colorize();
        self
    }
}

impl fmt::Display for WorkspaceOpsDisplay<'_, '_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let workspace_root = self.ops.graph.workspace().root();
        let workspace_root_manifest = workspace_root.join("Cargo.toml");
        for op in &self.ops.ops {
            match op {
                WorkspaceOp::NewCrate {
                    crate_path,
                    files,
                    root_files,
                } => {
                    write!(
                        f,
                        "* {} at {}",
                        "create crate".style(self.styles.create_bold_style),
                        crate_path.style(self.styles.create_bold_style),
                    )?;
                    if !files.is_empty() {
                        writeln!(f, ", with files:")?;
                        for file in files.keys() {
                            writeln!(f, "   - {}", file.style(self.styles.create_style))?;
                        }
                    } else {
                        writeln!(f)?;
                    }
                    writeln!(
                        f,
                        "* {} at {} to {}",
                        "add crate".style(self.styles.add_bold_style),
                        crate_path.style(self.styles.add_style),
                        workspace_root_manifest.style(self.styles.add_to_style),
                    )?;
                    if !root_files.is_empty() {
                        writeln!(
                            f,
                            "* {} at workspace root:",
                            "create files".style(self.styles.create_bold_style)
                        )?;
                        for file in root_files.keys() {
                            writeln!(f, "   - {}", file.style(self.styles.create_style))?;
                        }
                    }
                }
                WorkspaceOp::AddDependency {
                    name,
                    version,
                    crate_path,
                    dep_format: _,
                    line_style: _,
                    add_to,
                } => {
                    writeln!(
                        f,
                        "* {} {} v{} (at path {}) to packages:",
                        "add or update dependency".style(self.styles.add_bold_style),
                        name.style(self.styles.add_style),
                        version.style(self.styles.add_style),
                        crate_path.style(self.styles.add_style),
                    )?;
                    for (name, path) in package_names_paths(add_to) {
                        writeln!(
                            f,
                            "   - {} (at path {})",
                            name.style(self.styles.add_to_bold_style),
                            path.style(self.styles.add_to_style)
                        )?;
                    }
                }
                WorkspaceOp::RemoveDependency { name, remove_from } => {
                    writeln!(
                        f,
                        "* {} {} from packages:",
                        "remove dependency".style(self.styles.remove_bold_style),
                        name.style(self.styles.remove_style),
                    )?;
                    for (name, path) in package_names_paths(remove_from) {
                        writeln!(
                            f,
                            "   - {} (at path {})",
                            name.style(self.styles.remove_from_bold_style),
                            path.style(self.styles.remove_from_style)
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct Styles {
    create_style: Style,
    add_style: Style,
    add_to_style: Style,
    remove_style: Style,
    remove_from_style: Style,
    create_bold_style: Style,
    add_bold_style: Style,
    add_to_bold_style: Style,
    remove_bold_style: Style,
    remove_from_bold_style: Style,
}

impl Styles {
    fn colorize(&mut self) {
        self.create_style = Style::new().green();
        self.add_style = Style::new().cyan();
        self.add_to_style = Style::new().blue();
        self.remove_style = Style::new().red();
        self.remove_from_style = Style::new().purple();
        self.create_bold_style = self.create_style.bold();
        self.add_bold_style = self.add_style.bold();
        self.add_to_bold_style = self.add_to_style.bold();
        self.remove_bold_style = self.remove_style.bold();
        self.remove_from_bold_style = self.remove_from_style.bold();
    }
}

fn package_names_paths<'g>(package_set: &PackageSet<'g>) -> Vec<(&'g str, &'g Utf8Path)> {
    let mut package_names_paths: Vec<_> = package_set
        .packages(DependencyDirection::Forward)
        .map(|package| {
            (
                package.name(),
                package
                    .source()
                    .workspace_path()
                    .expect("workspace package"),
            )
        })
        .collect();
    package_names_paths.sort_unstable();
    package_names_paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use fixtures::json::JsonFixture;
    use std::iter;

    fn hack_dep() -> InlineTable {
        WorkspaceOp::inline_table_for_add(
            &"0.1.0".parse().expect("valid version"),
            DepFormatVersion::V4,
            WorkspaceHackLineStyle::Full,
            "../workspace-hack".into(),
        )
    }

    fn parse(toml: &str) -> DocumentMut {
        toml.parse().expect("test manifest is valid TOML")
    }

    fn canonical_manifest_dir() -> Utf8PathBuf {
        Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize_utf8()
            .expect("crate directory is canonicalized")
    }

    fn file_map(path: &str) -> BTreeMap<Cow<'_, Utf8Path>, Cow<'_, [u8]>> {
        BTreeMap::from([(Cow::Borrowed(Utf8Path::new(path)), Cow::Borrowed(&[][..]))])
    }

    fn io_error() -> io::Error {
        io::Error::other("io error")
    }

    #[test]
    fn add_dependency_creates_section() {
        let mut doc = parse(
            r#"[package]
name = "foo"
"#,
        );
        add_dependency_to_document(&mut doc, "workspace-hack", hack_dep())
            .expect("[dependencies] is created");
        assert_eq!(
            doc.to_string(),
            r#"[package]
name = "foo"

[dependencies]
workspace-hack = { version = "0.1", path = "../workspace-hack" }
"#,
        );
    }

    #[test]
    fn add_dependency_replaces_existing_line() {
        let mut doc = parse(
            r#"[dependencies]
workspace-hack = { path = "../old" }
other = "1"
"#,
        );
        add_dependency_to_document(&mut doc, "workspace-hack", hack_dep())
            .expect("[dependencies] is a table");
        assert_eq!(
            doc.to_string(),
            r#"[dependencies]
workspace-hack = { version = "0.1", path = "../workspace-hack" }
other = "1"
"#,
            "the existing line is replaced in place"
        );
    }

    #[test]
    fn add_dependency_drops_dev_dependency_line() {
        let mut doc = parse(
            r#"[package]
name = "foo"

[dev-dependencies]
workspace-hack = { path = "../workspace-hack" }
other = "1"

[build-dependencies]
workspace-hack = { path = "../workspace-hack" }
"#,
        );
        add_dependency_to_document(&mut doc, "workspace-hack", hack_dep())
            .expect("[dependencies] is created");
        assert_eq!(
            doc.to_string(),
            r#"[package]
name = "foo"

[dev-dependencies]
other = "1"

[build-dependencies]
workspace-hack = { path = "../workspace-hack" }

[dependencies]
workspace-hack = { version = "0.1", path = "../workspace-hack" }
"#,
            "the dev-dependency line is dropped, the build-dependency line is kept"
        );
    }

    #[test]
    fn add_dependency_rejects_non_table_dev_section() {
        let mut doc = parse(
            "dev-dependencies = 1
",
        );
        assert_eq!(
            add_dependency_to_document(&mut doc, "workspace-hack", hack_dep()),
            Err(NotATableError::Section {
                section: DependencySection::Dev
            }),
        );
    }

    #[test]
    fn add_dependency_rejects_non_table_section() {
        let mut doc = parse(
            "dependencies = 1
",
        );
        assert_eq!(
            add_dependency_to_document(&mut doc, "workspace-hack", hack_dep()),
            Err(NotATableError::Section {
                section: DependencySection::Normal
            }),
        );
    }

    #[test]
    fn remove_dependency_removes_line() {
        let mut doc = parse(
            r#"[dependencies]
workspace-hack = { path = "../workspace-hack" }
other = "1"
"#,
        );
        remove_dependency_from_document(&mut doc, "workspace-hack")
            .expect("[dependencies] is a table");
        assert_eq!(
            doc.to_string(),
            r#"[dependencies]
other = "1"
"#,
        );
    }

    #[test]
    fn remove_dependency_removes_from_every_section() {
        let mut doc = parse(
            r#"[dependencies]
workspace-hack = { path = "../workspace-hack" }

[dev-dependencies]
workspace-hack = { path = "../workspace-hack" }
other = "1"

[build-dependencies]
workspace-hack = { path = "../workspace-hack" }
"#,
        );
        remove_dependency_from_document(&mut doc, "workspace-hack")
            .expect("all sections are tables");
        assert_eq!(
            doc.to_string(),
            r#"[dependencies]

[dev-dependencies]
other = "1"

[build-dependencies]
"#,
        );
    }

    #[test]
    fn remove_dependency_removes_platform_specific_lines() {
        let mut doc = parse(
            r#"[dependencies]
other = "1"

[target.'cfg(windows)'.dependencies]
workspace-hack = { path = "../workspace-hack" }

[target.'cfg(unix)'.dependencies]
workspace-hack = { path = "../workspace-hack" }
other = "1"

[target.'cfg(unix)'.dev-dependencies]
workspace-hack = { path = "../workspace-hack" }

[target.'cfg(unix)'.build-dependencies]
workspace-hack = { path = "../workspace-hack" }
"#,
        );
        remove_dependency_from_document(&mut doc, "workspace-hack")
            .expect("all sections are tables");
        assert_eq!(
            doc.to_string(),
            r#"[dependencies]
other = "1"

[target.'cfg(windows)'.dependencies]

[target.'cfg(unix)'.dependencies]
other = "1"

[target.'cfg(unix)'.dev-dependencies]

[target.'cfg(unix)'.build-dependencies]
"#,
        );
    }

    #[test]
    fn remove_dependency_rejects_non_table_platform_section() {
        let mut doc = parse("target = 1\n");
        assert_eq!(
            remove_dependency_from_document(&mut doc, "workspace-hack"),
            Err(NotATableError::Target),
        );

        let mut doc = parse("[target]\n'cfg(unix)' = 1\n");
        assert_eq!(
            remove_dependency_from_document(&mut doc, "workspace-hack"),
            Err(NotATableError::Platform {
                platform: "cfg(unix)".to_owned()
            }),
        );

        let mut doc = parse("[target.'cfg(unix)']\ndev-dependencies = 1\n");
        assert_eq!(
            remove_dependency_from_document(&mut doc, "workspace-hack"),
            Err(NotATableError::PlatformSection {
                platform: "cfg(unix)".to_owned(),
                section: DependencySection::Dev,
            }),
        );
    }

    #[test]
    fn remove_dependency_without_section_is_noop() {
        let toml = r#"[package]
name = "foo"
"#;
        let mut doc = parse(toml);
        remove_dependency_from_document(&mut doc, "workspace-hack")
            .expect("missing sections are fine");
        assert_eq!(doc.to_string(), toml, "no sections are added");
    }

    #[test]
    fn remove_dependency_rejects_non_table_section() {
        let mut doc = parse(
            "build-dependencies = 1
",
        );
        assert_eq!(
            remove_dependency_from_document(&mut doc, "workspace-hack"),
            Err(NotATableError::Section {
                section: DependencySection::Build
            }),
        );
    }

    #[test]
    fn apply_rejects_missing_workspace_root() {
        let graph = JsonFixture::metadata1().graph();
        let error = WorkspaceOps::new(graph, [])
            .apply()
            .expect_err("the fixture's workspace root doesn't exist");
        assert_eq!(error.path(), graph.workspace().root());
        let ApplyErrorKind::CanonicalizeWorkspaceRoot { error: io_error } = error.kind() else {
            panic!("expected CanonicalizeWorkspaceRoot, found {error:?}");
        };
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn canonical_rel_path_returns_path_within_base() {
        let base = canonical_manifest_dir();
        let rel_path =
            canonical_rel_path("src".into(), &base).expect("src is within the crate directory");
        assert_eq!(rel_path, "src");
    }

    #[test]
    fn canonical_rel_path_rejects_missing_path() {
        let base = canonical_manifest_dir();
        let error =
            canonical_rel_path("does-not-exist".into(), &base).expect_err("the path doesn't exist");
        assert_eq!(error.path(), base.join("does-not-exist"));
        let ApplyErrorKind::CanonicalizePath { error: io_error } = error.kind() else {
            panic!("expected CanonicalizePath, found {error:?}");
        };
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn canonical_rel_path_rejects_path_outside_base() {
        let base = canonical_manifest_dir();
        let error =
            canonical_rel_path("..".into(), &base).expect_err("the parent is outside the base");
        assert_eq!(error.path(), base.join(".."));
        let ApplyErrorKind::PathOutsideBase {
            canonical_path,
            canonical_base,
        } = error.kind()
        else {
            panic!("expected PathOutsideBase, found {error:?}");
        };
        assert_eq!(
            canonical_path,
            base.parent().expect("crate directory has a parent"),
        );
        assert_eq!(canonical_base, &base);
    }

    #[test]
    fn create_new_crate_rejects_directory_under_file() {
        let base = canonical_manifest_dir();
        let error =
            WorkspaceOp::create_new_crate(&base, "Cargo.toml".into(), &file_map("src/lib.rs"))
                .expect_err("Cargo.toml is a file, so no directory can be created under it");
        assert_eq!(error.path(), base.join("Cargo.toml").join("src"));
        let ApplyErrorKind::CreateDirs { error: _ } = error.kind() else {
            panic!("expected CreateDirs, found {error:?}");
        };
    }

    #[test]
    fn create_new_crate_rejects_missing_file_name() {
        let base = canonical_manifest_dir();
        let error = WorkspaceOp::create_new_crate(&base, "src".into(), &file_map(".."))
            .expect_err("`..` has no file name");
        assert_eq!(error.path(), "..");
        let ApplyErrorKind::MissingFileName = error.kind() else {
            panic!("expected MissingFileName, found {error:?}");
        };
    }

    #[test]
    fn read_toml_rejects_missing_manifest() {
        let manifest_path = canonical_manifest_dir()
            .join("does-not-exist")
            .join("Cargo.toml");
        let error = read_toml(&manifest_path).expect_err("the manifest doesn't exist");
        assert_eq!(error.path(), manifest_path);
        let ApplyErrorKind::ReadManifest { error: io_error } = error.kind() else {
            panic!("expected ReadManifest, found {error:?}");
        };
        assert_eq!(io_error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn parse_toml_rejects_invalid_toml() {
        let error =
            parse_toml("[workspace", "Cargo.toml".into()).expect_err("the table header is open");
        assert_eq!(error.path(), "Cargo.toml");
        let ApplyErrorKind::ParseManifest { error: _ } = error.kind() else {
            panic!("expected ParseManifest, found {error:?}");
        };
    }

    #[test]
    fn write_contents_rejects_missing_parent() {
        let path = canonical_manifest_dir()
            .join("does-not-exist")
            .join("file.txt");
        let error = write_contents(&[], &path).expect_err("the parent directory doesn't exist");
        assert_eq!(error.path(), path);
        let ApplyErrorKind::WriteFile { error: _ } = error.kind() else {
            panic!("expected WriteFile, found {error:?}");
        };
    }

    #[test]
    fn workspace_members_rejects_missing_workspace_section() {
        let mut doc = parse(
            r#"[package]
name = "foo"
"#,
        );
        assert_eq!(
            WorkspaceOp::get_workspace_members_array(&mut doc).expect_err("[workspace] is missing"),
            WorkspaceSectionError::NotFound,
        );
    }

    #[test]
    fn workspace_members_rejects_non_table_workspace() {
        for (toml, expected_found) in [
            ("workspace = 1\n", "integer"),
            ("workspace = { members = [] }\n", "inline table"),
        ] {
            let mut doc = parse(toml);
            assert_eq!(
                WorkspaceOp::get_workspace_members_array(&mut doc)
                    .expect_err("[workspace] is not a table"),
                WorkspaceSectionError::NotATable {
                    found: expected_found
                },
                "for manifest {toml:?}",
            );
        }
    }

    #[test]
    fn workspace_members_rejects_missing_members() {
        let mut doc = parse("[workspace]\n");
        assert_eq!(
            WorkspaceOp::get_workspace_members_array(&mut doc)
                .expect_err("workspace.members is missing"),
            WorkspaceSectionError::MembersNotFound,
        );
    }

    #[test]
    fn workspace_members_rejects_non_array_members() {
        for (toml, expected_found) in [
            ("[workspace]\nmembers = \"foo\"\n", "string"),
            ("[workspace]\nmembers = { foo = 1 }\n", "inline table"),
            ("[workspace.members]\nfoo = 1\n", "table"),
            ("[[workspace.members]]\nfoo = 1\n", "array of tables"),
        ] {
            let mut doc = parse(toml);
            assert_eq!(
                WorkspaceOp::get_workspace_members_array(&mut doc)
                    .expect_err("workspace.members is not an array"),
                WorkspaceSectionError::MembersNotAnArray {
                    found: expected_found
                },
                "for manifest {toml:?}",
            );
        }
    }

    #[test]
    fn add_to_workspace_members_rejects_non_string_member() {
        let mut doc = parse(
            r#"[workspace]
members = ["a", 1]
"#,
        );
        assert_eq!(
            WorkspaceOp::add_to_workspace_members(&mut doc, "b".into()),
            Err(WorkspaceSectionError::MemberNotAString {
                index: 1,
                found: "integer",
            }),
        );
    }

    #[test]
    fn add_to_workspace_members_inserts_in_order() {
        let mut doc = parse(
            r#"[workspace]
members = [
    "a",
    "c",
]
"#,
        );
        WorkspaceOp::add_to_workspace_members(&mut doc, "b".into())
            .expect("workspace.members is an array of strings");
        assert_eq!(
            doc.to_string(),
            r#"[workspace]
members = [
    "a",
    "b",
    "c",
]
"#,
        );
    }

    #[test]
    fn add_to_workspace_members_appends_after_last_member() {
        let mut doc = parse(
            r#"[workspace]
members = [
    "a",
    "b",
]
"#,
        );
        WorkspaceOp::add_to_workspace_members(&mut doc, "c".into())
            .expect("workspace.members is an array of strings");
        assert_eq!(
            doc.to_string(),
            r#"[workspace]
members = [
    "a",
    "b",
    "c",
]
"#,
        );
    }

    #[test]
    fn add_to_workspace_members_skips_existing_member() {
        let toml = r#"[workspace]
members = [
    "a",
    "b",
]
"#;
        let mut doc = parse(toml);
        WorkspaceOp::add_to_workspace_members(&mut doc, "b".into())
            .expect("workspace.members is an array of strings");
        assert_eq!(doc.to_string(), toml, "the existing member is not repeated");
    }

    #[test]
    fn apply_error_display_and_source() {
        let toml_error = "["
            .parse::<DocumentMut>()
            .expect_err("the table header is open");
        let cases = [
            (
                ApplyErrorKind::CanonicalizeWorkspaceRoot { error: io_error() },
                "for path Cargo.toml, unable to canonicalize workspace root",
                Some(io_error().to_string()),
            ),
            (
                ApplyErrorKind::CanonicalizePath { error: io_error() },
                "for path Cargo.toml, error canonicalizing path",
                Some(io_error().to_string()),
            ),
            (
                ApplyErrorKind::PathOutsideBase {
                    canonical_path: "/elsewhere/Cargo.toml".into(),
                    canonical_base: "/workspace".into(),
                },
                "for path Cargo.toml, canonical path is not within base path /workspace",
                None,
            ),
            (
                ApplyErrorKind::CreateDirs { error: io_error() },
                "for path Cargo.toml, error creating directories",
                Some(io_error().to_string()),
            ),
            (
                ApplyErrorKind::MissingFileName,
                "for path Cargo.toml, does not contain a file name",
                None,
            ),
            (
                ApplyErrorKind::ReadManifest { error: io_error() },
                "for path Cargo.toml, error reading TOML file",
                Some(io_error().to_string()),
            ),
            (
                ApplyErrorKind::ParseManifest {
                    error: toml_error.clone(),
                },
                "for path Cargo.toml, error deserializing TOML file",
                Some(toml_error.to_string()),
            ),
            (
                ApplyErrorKind::WriteFile { error: io_error() },
                "for path Cargo.toml, error writing file",
                Some(io_error().to_string()),
            ),
            (
                ApplyErrorKind::WorkspaceSection {
                    error: WorkspaceSectionError::NotFound,
                },
                "for path Cargo.toml, [workspace] section not found",
                None,
            ),
            (
                ApplyErrorKind::WorkspaceSection {
                    error: WorkspaceSectionError::NotATable { found: "integer" },
                },
                "for path Cargo.toml, expected [workspace] to be a table, found integer",
                None,
            ),
            (
                ApplyErrorKind::WorkspaceSection {
                    error: WorkspaceSectionError::MembersNotFound,
                },
                "for path Cargo.toml, workspace.members not found",
                None,
            ),
            (
                ApplyErrorKind::WorkspaceSection {
                    error: WorkspaceSectionError::MembersNotAnArray { found: "string" },
                },
                "for path Cargo.toml, expected workspace.members to be an array, found string",
                None,
            ),
            (
                ApplyErrorKind::WorkspaceSection {
                    error: WorkspaceSectionError::MemberNotAString {
                        index: 1,
                        found: "integer",
                    },
                },
                "for path Cargo.toml, workspace.members contains non-strings",
                None,
            ),
            (
                ApplyErrorKind::NotATable {
                    error: NotATableError::Section {
                        section: DependencySection::Normal,
                    },
                },
                "for path Cargo.toml, [dependencies] is not a table",
                None,
            ),
            (
                ApplyErrorKind::NotATable {
                    error: NotATableError::Target,
                },
                "for path Cargo.toml, [target] is not a table",
                None,
            ),
            (
                ApplyErrorKind::NotATable {
                    error: NotATableError::Platform {
                        platform: "cfg(unix)".to_owned(),
                    },
                },
                "for path Cargo.toml, [target.'cfg(unix)'] is not a table",
                None,
            ),
            (
                ApplyErrorKind::NotATable {
                    error: NotATableError::PlatformSection {
                        platform: "cfg(unix)".to_owned(),
                        section: DependencySection::Dev,
                    },
                },
                "for path Cargo.toml, [target.'cfg(unix)'.dev-dependencies] is not a table",
                None,
            ),
        ];

        for (kind, expected_display, expected_source) in cases {
            let error = ApplyError::new("Cargo.toml", kind);
            assert_eq!(error.to_string(), expected_display);
            let chain: Vec<String> =
                iter::successors(Some(&error as &dyn error::Error), |error| error.source())
                    .map(|error| error.to_string())
                    .collect();
            let mut expected_chain = vec![expected_display.to_owned()];
            expected_chain.extend(expected_source);
            assert_eq!(
                chain, expected_chain,
                "each message appears once in the source chain",
            );
        }
    }

    #[test]
    fn test_inline_table_for_add() {
        let versions = vec![
            ("1.2.3", "1", "1"),
            ("1.2.3-a.1+g456", "1.2.3-a.1+g456", "1.2.3-a.1"),
        ];

        for (version, version_str, version_str_v3) in versions {
            let version: Version = version.parse().unwrap();
            let itable = WorkspaceOp::inline_table_for_add(
                &version,
                DepFormatVersion::V1,
                WorkspaceHackLineStyle::Full,
                "../../path".into(),
            );
            assert_eq!(
                itable.to_string(),
                "{ path = \"../../path\"}",
                "dep format v1 matches"
            );

            let itable = WorkspaceOp::inline_table_for_add(
                &version,
                DepFormatVersion::V2,
                WorkspaceHackLineStyle::Full,
                "../../path".into(),
            );
            assert_eq!(
                itable.to_string(),
                format!("{{ version = \"{version_str}\", path = \"../../path\" }}"),
                "dep format v2 matches"
            );

            let itable = WorkspaceOp::inline_table_for_add(
                &version,
                DepFormatVersion::V3,
                WorkspaceHackLineStyle::Full,
                "../../path".into(),
            );
            assert_eq!(
                itable.to_string(),
                format!("{{ version = \"{version_str_v3}\", path = \"../../path\" }}"),
                "dep format v3 matches"
            );

            let itable = WorkspaceOp::inline_table_for_add(
                &version,
                DepFormatVersion::V4,
                WorkspaceHackLineStyle::VersionOnly,
                "../../path".into(),
            );
            assert_eq!(
                itable.to_string(),
                format!("{{ version = \"{version_str_v3}\" }}"),
                "version only matches"
            );

            let itable = WorkspaceOp::inline_table_for_add(
                &version,
                DepFormatVersion::V4,
                WorkspaceHackLineStyle::WorkspaceDotted,
                "../../path".into(),
            );
            let mut document = DocumentMut::new();
            document
                .as_table_mut()
                .insert("workspace-hack", Item::Value(Value::InlineTable(itable)));
            assert_eq!(
                document.to_string(),
                "workspace-hack.workspace = true\n",
                "workspace dep matches"
            );
        }
    }
}
