//! Schema resolution utilities.
//!
//! This module provides high-level functions to resolve and compile schemas
//! from XML documents that contain `xsi:schemaLocation` attributes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Result;
use crate::schema::export::{ExportResult, export_schemas_from_xml};
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::types::CompiledSchema;
use crate::schema::xsd::{create_builtin_schema, parse_xsd_multiple};

/// Result of schema resolution.
#[derive(Debug)]
pub struct ResolvedSchema {
    /// The compiled schema ready for validation.
    pub compiled: Arc<CompiledSchema>,
    /// Directory containing exported schema files (if schemas were exported).
    pub export_dir: Option<PathBuf>,
    /// The entry schema filename in export_dir.
    pub entry_filename: Option<String>,
    /// Export result with URI to filename mappings.
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

    /// Returns true if this is a builtin schema (no external schemas were resolved).
    pub fn is_builtin(&self) -> bool {
        self.export_dir.is_none()
    }
}

/// Options for schema resolution.
#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    /// Base directory for resolving relative schema paths.
    /// If None, relative paths in the XML will be resolved from current directory.
    pub base_dir: Option<PathBuf>,
    /// Custom export directory. If None, a temp directory will be used.
    pub export_dir: Option<PathBuf>,
    /// Whether to keep the export directory after resolution.
    /// If false (default), the directory may be cleaned up.
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
/// 1. Parses the XML to extract `xsi:schemaLocation` attributes
/// 2. Fetches all referenced schemas (including imports/includes)
/// 3. Exports schemas to a directory with rewritten paths
/// 4. Compiles all schemas into a single `CompiledSchema`
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
/// println!("Compiled {} types", resolved.compiled.types.len());
/// ```
pub fn resolve_schema_from_xml<F: SchemaFetcher>(
    xml_content: &[u8],
    fetcher: &F,
    options: &ResolveOptions,
) -> Result<ResolvedSchema> {
    // Determine export directory
    let export_dir = options.export_dir.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("fastxml_schemas_{}", std::process::id()))
    });

    // Clean up previous export if exists
    let _ = std::fs::remove_dir_all(&export_dir);

    // Export schemas
    let export_result = export_schemas_from_xml(xml_content, &export_dir, fetcher)?;

    if export_result.schema_count == 0 {
        return Ok(ResolvedSchema {
            compiled: Arc::new(create_builtin_schema()),
            export_dir: None,
            entry_filename: None,
            export_result: None,
        });
    }

    // Read and compile all exported schemas
    let mut xsd_contents: Vec<(String, Vec<u8>)> = Vec::new();
    for (uri, filename) in &export_result.uri_to_filename {
        let path = export_dir.join(filename);
        if let Ok(content) = std::fs::read(&path) {
            xsd_contents.push((uri.clone(), content));
        }
    }

    let xsd_refs: Vec<(&str, &[u8])> = xsd_contents
        .iter()
        .map(|(uri, content)| (uri.as_str(), content.as_slice()))
        .collect();

    let compiled = match parse_xsd_multiple(&xsd_refs) {
        Ok(schema) => Arc::new(schema),
        Err(_) => {
            // Fall back to builtin schema on parse error
            Arc::new(create_builtin_schema())
        }
    };

    Ok(ResolvedSchema {
        compiled,
        export_dir: Some(export_dir),
        entry_filename: export_result.entry_filename.clone(),
        export_result: Some(export_result),
    })
}

/// Resolves schemas from an XML file path.
///
/// This is a convenience function that reads the file and uses its parent
/// directory as the base for resolving relative schema paths.
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

    let options = if let Some(parent) = path.parent() {
        ResolveOptions::with_base_dir(parent)
    } else {
        ResolveOptions::default()
    };

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
