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
            ApplyError::io(
                "unable to canonicalize workspace root",
                workspace_root.to_owned(),
                error,
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
                    std::fs::create_dir_all(parent)
                        .map_err(|err| ApplyError::io("error creating directories", parent, err))?;
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
            std::fs::create_dir_all(&dir_path)
                .map_err(|err| ApplyError::io("error creating directories", &dir_path, err))?;

            // Write out the file.
            dir_path.push(
                path.file_name().ok_or_else(|| {
                    ApplyError::misc("does not contain a file name", path.as_ref())
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
        let members = Self::get_workspace_members_array(&root_toml_path, &mut doc)?;

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
                    return Err(ApplyError::misc(
                        "workspace.members contains non-strings",
                        root_toml_path,
                    ));
                }
            }
        }

        if !written {
            add(members, members.len());
        }

        write_document(&doc, &root_toml_path)
    }

    fn get_workspace_members_array<'doc>(
        root_toml_path: &Utf8Path,
        doc: &'doc mut DocumentMut,
    ) -> Result<&'doc mut Array, ApplyError> {
        let doc_table = doc.as_table_mut();
        let workspace_table = match doc_table.get_mut("workspace") {
            Some(Item::Table(workspace_table)) => workspace_table,
            Some(other) => {
                return Err(ApplyError::misc(
                    format!(
                        "expected [workspace] to be a table, found {}",
                        other.type_name()
                    ),
                    root_toml_path,
                ));
            }
            None => {
                return Err(ApplyError::misc(
                    "[workspace] section not found",
                    root_toml_path,
                ));
            }
        };

        let members = match workspace_table.get_mut("members") {
            Some(Item::Value(members)) => match members.as_array_mut() {
                Some(members) => members,
                None => {
                    return Err(ApplyError::misc(
                        "workspace.members is not an array",
                        root_toml_path,
                    ));
                }
            },
            Some(other) => {
                return Err(ApplyError::misc(
                    format!(
                        "expected workspace.members to be an array, found {}",
                        other.type_name()
                    ),
                    root_toml_path,
                ));
            }
            None => {
                return Err(ApplyError::misc(
                    "workspace.members not found",
                    root_toml_path,
                ));
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
        let dep_table = Self::get_or_insert_dependencies_table(manifest_path, &mut doc)?;

        let package_path = package
            .source()
            .workspace_path()
            .expect("package should be in workspace");
        // Find the location of the new path (relative) with respect to the package path.
        let path = pathdiff::diff_utf8_paths(crate_path, package_path)
            .expect("both new_path and package_path are relative");

        let path_table = Self::inline_table_for_add(version, dep_format, line_style, &path);

        dep_table.insert(name, Item::Value(Value::InlineTable(path_table)));

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
        let dep_table = Self::get_or_insert_dependencies_table(manifest_path, &mut doc)?;
        // TODO: someone might have added the workspace-hack package under a different name.
        // Handle that if someone complains.
        dep_table.remove(name);

        write_document(&doc, manifest_path)
    }

    fn get_or_insert_dependencies_table<'doc>(
        manifest_path: &Utf8Path,
        doc: &'doc mut DocumentMut,
    ) -> Result<&'doc mut dyn TableLike, ApplyError> {
        let doc_table = doc.as_table_mut();

        if doc_table.contains_key("dependencies") {
            match doc_table
                .get_mut("dependencies")
                .expect("just checked for presence of dependencies")
                .as_table_like_mut()
            {
                Some(table) => Ok(table),
                None => Err(ApplyError::misc(
                    "[dependencies] is not a table",
                    manifest_path,
                )),
            }
        } else {
            // Add the dependencies table.
            let mut new_table = Table::new();
            new_table.set_implicit(true);
            doc_table.insert("dependencies", Item::Table(new_table));
            let table = doc_table
                .get_mut("dependencies")
                .expect("was just inserted")
                .as_table_like_mut()
                .expect("was just inserted");
            Ok(table)
        }
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
        .map_err(|err| ApplyError::io("error canonicalizing path", &abs_path, err))?;
    canonical_path
        .strip_prefix(canonical_base)
        .map_err(|_| {
            // This can happen under some symlink scenarios.
            ApplyError::misc(
                format!("canonical path is not within base path {canonical_base}"),
                &abs_path,
            )
        })
        .map(|p| p.to_owned())
}

// ---
// Read/write functions
// ---

fn read_toml(manifest_path: &Utf8Path) -> Result<DocumentMut, ApplyError> {
    let toml = fs::read_to_string(manifest_path)
        .map_err(|err| ApplyError::io("error reading TOML file", manifest_path, err))?;
    toml.parse::<DocumentMut>()
        .map_err(|err| ApplyError::toml("error deserializing TOML file", manifest_path, err))
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
        Err(atomicwrites::Error::Internal(err)) | Err(atomicwrites::Error::User(err)) => {
            Err(ApplyError::io("error writing file", path, err))
        }
    }
}

/// An error that occurred while writing out changes to a workspace.
#[derive(Debug)]
pub struct ApplyError {
    message: String,
    path: Utf8PathBuf,
    kind: Box<ApplyErrorKind>,
}

impl ApplyError {
    /// Returns the message corresponding to the error.
    #[inline]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the path at which the error occurred.
    #[inline]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    // ---
    // Helper methods
    // ---
    fn io(message: impl Into<String>, path: impl Into<Utf8PathBuf>, err: io::Error) -> Self {
        Self {
            message: message.into(),
            path: path.into(),
            kind: Box::new(ApplyErrorKind::Io { err }),
        }
    }

    fn toml(
        message: impl Into<String>,
        path: impl Into<Utf8PathBuf>,
        err: toml_edit::TomlError,
    ) -> Self {
        Self {
            message: message.into(),
            path: path.into(),
            kind: Box::new(ApplyErrorKind::Toml { err }),
        }
    }

    fn misc(message: impl Into<String>, path: impl Into<Utf8PathBuf>) -> Self {
        Self {
            message: message.into(),
            path: path.into(),
            kind: Box::new(ApplyErrorKind::Misc),
        }
    }
}

impl fmt::Display for ApplyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "for path {}, {}", self.path, self.message)
    }
}

impl error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match &*self.kind {
            ApplyErrorKind::Io { err } => Some(err),
            ApplyErrorKind::Toml { err } => Some(err),
            ApplyErrorKind::Misc => None,
        }
    }
}

#[derive(Debug)]
enum ApplyErrorKind {
    Io { err: io::Error },
    Toml { err: TomlError },
    Misc,
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
