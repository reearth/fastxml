//! Schema resolution utilities.
//!
//! This module provides high-level functions to resolve and compile schemas
//! from XML documents that contain `xsi:schemaLocation` /
//! `xsi:noNamespaceSchemaLocation` attributes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::schema::export::{ExportResult, SchemaSet, hint_locations};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::hints::SchemaHints;
use crate::schema::types::CompiledSchema;
use crate::schema::xsd::{create_builtin_schema, parse_xsd_multiple};

/// Result of schema resolution.
#[derive(Debug)]
pub struct ResolvedSchema {
    /// The compiled schema ready for validation.
    pub compiled: Arc<CompiledSchema>,
    /// Directory holding the exported schema files, when they were written
    /// to disk and kept (see [`ResolveOptions`]).
    pub export_dir: Option<PathBuf>,
    /// The entry schema's filename in the export (inside `export_dir` when
    /// that is set).
    pub entry_filename: Option<String>,
    /// URI to filename mappings of the resolved schema set; `None` when the
    /// document named no schema.
    pub export_result: Option<ExportResult>,
}

impl ResolvedSchema {
    /// Returns the path to the entry schema file, if available.
    pub fn entry_schema_path(&self) -> Option<PathBuf> {
        match (&self.export_dir, &self.entry_filename) {
            (Some(dir), Some(filename)) => Some(dir.join(filename)),
            _ => None,
        }
    }

    /// Returns true if this is a builtin schema (the document named no
    /// schema, so no external schemas were resolved).
    pub fn is_builtin(&self) -> bool {
        self.export_result
            .as_ref()
            .is_none_or(|r| r.schema_count == 0)
    }
}

/// Options for schema resolution.
///
/// Where the exported schema files go:
///
/// | `export_dir` | `keep_export_dir` | files on disk |
/// |---|---|---|
/// | `Some(dir)` | (ignored) | written into `dir`, which must be new or empty; never deleted |
/// | `None` | `true` | written into a fresh, uniquely named temporary directory that is kept (the caller deletes it) |
/// | `None` | `false` (default) | none — the schemas are compiled in memory and `ResolvedSchema::export_dir` is `None` |
#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    /// Base directory for resolving relative schema locations named by the
    /// document (typically the document's own directory). If None, relative
    /// locations are resolved by the fetcher (against its base directory, or
    /// else the current directory).
    pub base_dir: Option<PathBuf>,
    /// Directory to export the schema files into. It is created if missing
    /// and must otherwise be empty: resolution refuses a non-empty directory
    /// rather than overwrite or delete anything in it.
    pub export_dir: Option<PathBuf>,
    /// With no `export_dir`, write the files into a new uniquely named
    /// temporary directory and keep it. Without this (the default) no files
    /// are written.
    pub keep_export_dir: bool,
}

impl ResolveOptions {
    /// Create options with a base directory for resolving relative paths.
    pub fn with_base_dir(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: Some(base_dir.as_ref().to_path_buf()),
            ..Default::default()
        }
    }

    /// Set a custom export directory.
    pub fn export_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.export_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Keep the export directory after resolution.
    pub fn keep_export_dir(mut self) -> Self {
        self.keep_export_dir = true;
        self
    }
}

/// Resolves and compiles schemas from an XML document.
///
/// This function:
/// 1. Reads the root element's `xsi:schemaLocation` /
///    `xsi:noNamespaceSchemaLocation` hints
/// 2. Fetches all referenced schemas (including imports/includes/redefines)
/// 3. Compiles them into a single `CompiledSchema`
/// 4. Optionally exports them with rewritten paths (see [`ResolveOptions`])
///
/// A document without hints yields the built-in schema
/// ([`ResolvedSchema::is_builtin`] is true).
///
/// # Errors
///
/// Returns an error when a hint is malformed, when any referenced schema
/// cannot be fetched or parsed, when the schemas do not compile, or when
/// `export_dir` is not empty. It never falls back to the built-in schema in
/// those cases.
///
/// # Arguments
///
/// * `xml_content` - The XML document content
/// * `fetcher` - Schema fetcher for downloading schemas
/// * `options` - Resolution options (base directory, export directory, etc.)
///
/// # Returns
///
/// A `ResolvedSchema` containing the compiled schema and export information.
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::resolve::{resolve_schema_from_xml, ResolveOptions};
/// use fastxml::schema::DefaultFetcher;
///
/// let xml = std::fs::read("document.xml")?;
/// let fetcher = DefaultFetcher::new();
/// let options = ResolveOptions::with_base_dir("./schemas");
///
/// let resolved = resolve_schema_from_xml(&xml, &fetcher, &options)?;
/// println!("Compiled {} types", resolved.compiled.types_ns.len());
/// ```
pub fn resolve_schema_from_xml<F: SchemaFetcher>(
    xml_content: &[u8],
    fetcher: &F,
    options: &ResolveOptions,
) -> Result<ResolvedSchema> {
    let hints = SchemaHints::from_xml_bytes(xml_content)?;
    if hints.is_empty() {
        return Ok(ResolvedSchema {
            compiled: Arc::new(create_builtin_schema()),
            export_dir: None,
            entry_filename: None,
            export_result: None,
        });
    }

    // Refuse a non-empty export directory before doing any work.
    if let Some(dir) = &options.export_dir {
        ensure_new_or_empty(dir)?;
    }

    let locations = hint_locations(&hints, |loc| {
        resolve_against_base(loc, options.base_dir.as_deref())
    })?;
    let set = SchemaSet::collect(&locations, fetcher)?;
    let sources = set.sources();
    let compiled = parse_xsd_multiple(
        &sources
            .iter()
            .map(|(uri, content)| (*uri, content.as_ref()))
            .collect::<Vec<_>>(),
    )?;

    let export_dir = match (&options.export_dir, options.keep_export_dir) {
        (Some(dir), _) => {
            set.write_to(dir)?;
            Some(dir.clone())
        }
        (None, true) => {
            let dir = tempfile::Builder::new()
                .prefix("fastxml_schemas_")
                .tempdir()?;
            set.write_to(dir.path())?;
            Some(dir.keep())
        }
        (None, false) => None,
    };

    Ok(ResolvedSchema {
        compiled: Arc::new(compiled),
        export_dir,
        entry_filename: set.entry_filename(),
        export_result: Some(set.result()),
    })
}

/// Errors unless `dir` is missing or an empty directory.
fn ensure_new_or_empty(dir: &Path) -> Result<()> {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(Error::InvalidOperation(format!(
                    "export_dir {} is not empty; schemas are only exported into a new or \
                     empty directory so that no existing file is overwritten or deleted",
                    dir.display()
                )));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Joins a relative, non-URL `location` onto `base_dir` (made absolute).
fn resolve_against_base(location: &str, base_dir: Option<&Path>) -> String {
    let is_url = location.contains("://");
    match base_dir {
        Some(base) if !is_url && !Path::new(location).is_absolute() => {
            let joined = base.join(location);
            std::path::absolute(&joined)
                .unwrap_or(joined)
                .display()
                .to_string()
        }
        _ => location.to_string(),
    }
}

/// Resolves schemas from an XML file path.
///
/// This is a convenience function that reads the file and uses its parent
/// directory as the base for resolving relative schema locations. No files
/// are exported; use [`resolve_schema_from_xml`] with [`ResolveOptions`] for
/// that.
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::resolve::resolve_schema_from_file;
/// use fastxml::schema::DefaultFetcher;
///
/// let fetcher = DefaultFetcher::new();
/// let resolved = resolve_schema_from_file("document.xml", &fetcher)?;
/// ```
pub fn resolve_schema_from_file<F: SchemaFetcher>(
    xml_path: impl AsRef<Path>,
    fetcher: &F,
) -> Result<ResolvedSchema> {
    let path = xml_path.as_ref();
    let content = std::fs::read(path)?;

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let options = ResolveOptions::with_base_dir(parent);

    resolve_schema_from_xml(&content, fetcher, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fetcher::NoopFetcher;

    #[test]
    fn test_resolve_no_schema_location() {
        let xml = br#"<?xml version="1.0"?><root>content</root>"#;
        let fetcher = NoopFetcher;
        let options = ResolveOptions::default();

        let result = resolve_schema_from_xml(xml, &fetcher, &options).unwrap();

        assert!(result.is_builtin());
        assert!(result.export_dir.is_none());
        assert!(result.entry_filename.is_none());
    }

    #[test]
    fn test_resolve_options_builder() {
        let options = ResolveOptions::with_base_dir("/some/path")
            .export_dir("/export/dir")
            .keep_export_dir();

        assert_eq!(options.base_dir, Some(PathBuf::from("/some/path")));
        assert_eq!(options.export_dir, Some(PathBuf::from("/export/dir")));
        assert!(options.keep_export_dir);
    }

    #[test]
    fn test_entry_schema_path() {
        let resolved = ResolvedSchema {
            compiled: Arc::new(create_builtin_schema()),
            export_dir: Some(PathBuf::from("/tmp/schemas")),
            entry_filename: Some("main.xsd".to_string()),
            export_result: None,
        };

        assert_eq!(
            resolved.entry_schema_path(),
            Some(PathBuf::from("/tmp/schemas/main.xsd"))
        );
    }
}
