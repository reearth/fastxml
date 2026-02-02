//! Schema export utilities.
//!
//! This module provides functionality to resolve and export schemas
//! to a local directory with rewritten import/include paths.
//!
//! This is useful for tools like libxml that need all schemas
//! in a single directory with relative paths.

use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;
use crate::schema::fetcher::{FetchResult, SchemaFetcher};
use crate::schema::memory::InMemoryStore;
use crate::schema::store::SchemaStore;

/// Result of schema export operation.
#[derive(Debug, Clone)]
pub struct ExportResult {
    /// Number of schemas exported.
    pub schema_count: usize,
    /// Map of original URIs to local filenames.
    pub uri_to_filename: HashMap<String, String>,
    /// The entry schema filename (first schema).
    pub entry_filename: Option<String>,
}

/// Exports schemas from xsi:schemaLocation to a local directory.
///
/// This function:
/// 1. Parses the XML to extract xsi:schemaLocation
/// 2. Fetches all referenced schemas (including imports/includes)
/// 3. Rewrites import/include schemaLocation attributes to relative paths
/// 4. Writes all schemas to the output directory
///
/// # Arguments
///
/// * `xml_content` - The XML document content
/// * `output_dir` - Directory to write schemas to
/// * `fetcher` - Schema fetcher for downloading schemas
///
/// # Returns
///
/// Export result with schema count and filename mappings
///
/// # Example
///
/// ```ignore
/// use fastxml::schema::export::export_schemas_from_xml;
/// use fastxml::schema::DefaultFetcher;
///
/// let xml = std::fs::read("document.xml")?;
/// let fetcher = DefaultFetcher::new();
/// let result = export_schemas_from_xml(&xml, Path::new("./schemas"), &fetcher)?;
/// println!("Exported {} schemas", result.schema_count);
/// ```
pub fn export_schemas_from_xml<F: SchemaFetcher>(
    xml_content: &[u8],
    output_dir: &Path,
    fetcher: &F,
) -> Result<ExportResult> {
    use crate::parser::parse_schema_locations;

    // Parse XML to get schema locations
    let doc = crate::parse(xml_content)?;
    let locations = parse_schema_locations(&doc)?;

    if locations.is_empty() {
        return Ok(ExportResult {
            schema_count: 0,
            uri_to_filename: HashMap::new(),
            entry_filename: None,
        });
    }

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)?;

    // Use InMemoryStore to collect all schemas
    let store = InMemoryStore::new();
    let mut entry_uri = None;

    // Fetch and resolve all schemas
    for (_namespace, location) in &locations {
        match fetcher.fetch(location) {
            Ok(result) => {
                // Track the first successfully fetched schema as the entry point
                if entry_uri.is_none() {
                    entry_uri = Some(result.final_url.clone());
                }

                let _ = store.put(&result.final_url, &result.content);

                // Parse and resolve imports recursively
                let _ =
                    resolve_imports_recursive(&result.final_url, &result.content, fetcher, &store);
            }
            Err(_) => {
                // Skip schemas that can't be fetched
                continue;
            }
        }
    }

    // Build URI to filename mapping
    let mut uri_to_filename: HashMap<String, String> = HashMap::new();
    let mut existing_filenames: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let uris = store.list()?;

    for uri in &uris {
        let filename = uri_to_safe_filename(uri, &existing_filenames);
        existing_filenames.insert(filename.clone());
        uri_to_filename.insert(uri.clone(), filename);
    }

    // Rewrite and export each schema
    for uri in &uris {
        if let Some(content) = store.get(uri)? {
            let filename = uri_to_filename.get(uri).unwrap();
            let rewritten = rewrite_schema_locations(&content, uri, &uri_to_filename)?;
            let output_path = output_dir.join(filename);
            std::fs::write(&output_path, rewritten)?;
        }
    }

    let entry_filename = entry_uri.and_then(|uri| uri_to_filename.get(&uri).cloned());

    Ok(ExportResult {
        schema_count: uri_to_filename.len(),
        uri_to_filename,
        entry_filename,
    })
}

/// Recursively resolves imports and includes from a schema.
fn resolve_imports_recursive<F: SchemaFetcher, S: SchemaStore>(
    base_uri: &str,
    content: &[u8],
    fetcher: &F,
    store: &S,
) -> Result<()> {
    // Parse schema to find imports/includes
    let content_str = std::str::from_utf8(content).unwrap_or("");

    // Extract import schemaLocation attributes
    for location in extract_schema_locations(content_str) {
        let resolved_uri = resolve_uri(base_uri, &location)?;

        if !store.contains(&resolved_uri) {
            match fetcher.fetch(&resolved_uri) {
                Ok(FetchResult {
                    content: fetched_content,
                    final_url,
                    ..
                }) => {
                    let _ = store.put(&final_url, &fetched_content);
                    if final_url != resolved_uri {
                        let _ = store.put(&resolved_uri, &fetched_content);
                    }
                    // Recurse
                    resolve_imports_recursive(&final_url, &fetched_content, fetcher, store)?;
                }
                Err(_) => {
                    // Skip schemas that can't be fetched
                    continue;
                }
            }
        }
    }

    Ok(())
}

/// Extracts schemaLocation values from import/include elements.
fn extract_schema_locations(content: &str) -> Vec<String> {
    let mut locations = Vec::new();

    // Simple regex-like extraction for schemaLocation attributes
    // Matches: schemaLocation="..." or schemaLocation='...'
    let patterns = [r#"schemaLocation=""#, r#"schemaLocation='"#];

    for pattern in patterns {
        let quote = if pattern.ends_with('"') { '"' } else { '\'' };
        let mut remaining = content;

        while let Some(start) = remaining.find(pattern) {
            let after_pattern = &remaining[start + pattern.len()..];
            if let Some(end) = after_pattern.find(quote) {
                let location = &after_pattern[..end];
                // Skip xsi:schemaLocation (contains spaces for namespace-location pairs)
                if !location.contains(' ') && !location.is_empty() {
                    locations.push(location.to_string());
                }
                remaining = &after_pattern[end + 1..];
            } else {
                break;
            }
        }
    }

    locations
}

/// Rewrites schemaLocation attributes in a schema to use local filenames.
///
/// `base_uri` is the URI of the schema being rewritten, used to resolve relative paths.
fn rewrite_schema_locations(
    content: &[u8],
    base_uri: &str,
    uri_to_filename: &HashMap<String, String>,
) -> Result<Vec<u8>> {
    let content_str = std::str::from_utf8(content).unwrap_or("");
    let mut result = content_str.to_string();

    // First, rewrite absolute URIs directly
    for (uri, filename) in uri_to_filename {
        // Replace in schemaLocation="..."
        let old_double = format!(r#"schemaLocation="{}""#, uri);
        let new_double = format!(r#"schemaLocation="{}""#, filename);
        result = result.replace(&old_double, &new_double);

        // Replace in schemaLocation='...'
        let old_single = format!(r#"schemaLocation='{}'"#, uri);
        let new_single = format!(r#"schemaLocation='{}'"#, filename);
        result = result.replace(&old_single, &new_single);
    }

    // Now handle relative paths by resolving them and finding the matching filename
    // Extract all remaining schemaLocation values and try to resolve them
    let locations = extract_schema_locations(&result);
    for location in locations {
        // Skip if it's already just a filename (already rewritten)
        if !location.contains('/') && !location.contains('\\') {
            continue;
        }

        // Try to resolve this relative path against the base URI
        if let Ok(resolved) = resolve_uri(base_uri, &location) {
            // Look up in our mapping
            if let Some(filename) = uri_to_filename.get(&resolved) {
                // Replace this relative path with the filename
                let old_double = format!(r#"schemaLocation="{}""#, location);
                let new_double = format!(r#"schemaLocation="{}""#, filename);
                result = result.replace(&old_double, &new_double);

                let old_single = format!(r#"schemaLocation='{}'"#, location);
                let new_single = format!(r#"schemaLocation='{}'"#, filename);
                result = result.replace(&old_single, &new_single);
            }
        }
    }

    Ok(result.into_bytes())
}

/// Converts a URI to a safe filename, ensuring uniqueness.
fn uri_to_safe_filename(
    uri: &str,
    existing_filenames: &std::collections::HashSet<String>,
) -> String {
    // Remove protocol
    let without_protocol = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);

    // Get the last path component or use a hash
    let base_filename = Path::new(without_protocol)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Use hash for complex URIs
            format!("schema_{:x}.xsd", hash_uri(uri))
        });

    // Ensure .xsd extension
    let base_filename = if base_filename.ends_with(".xsd") {
        base_filename
    } else {
        format!("{}.xsd", base_filename)
    };

    // Make filename unique if it already exists
    if !existing_filenames.contains(&base_filename) {
        return base_filename;
    }

    // Add hash suffix to make it unique
    let stem = base_filename.strip_suffix(".xsd").unwrap_or(&base_filename);
    let hash_suffix = format!("{:08x}", hash_uri(uri) as u32);
    format!("{}_{}.xsd", stem, hash_suffix)
}

/// Simple hash function for URIs.
fn hash_uri(uri: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    uri.hash(&mut hasher);
    hasher.finish()
}

/// Resolves a relative URI against a base URI.
fn resolve_uri(base: &str, relative: &str) -> Result<String> {
    // If relative is already absolute, use it directly
    if relative.starts_with("http://")
        || relative.starts_with("https://")
        || relative.starts_with("file://")
    {
        return Ok(relative.to_string());
    }

    // Handle file:// base URIs
    if let Some(base_path) = base.strip_prefix("file://") {
        let base_dir = Path::new(base_path).parent().unwrap_or(Path::new("."));
        let resolved = base_dir.join(relative);
        let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
        return Ok(format!("file://{}", canonical.display()));
    }

    // Handle http(s) base URIs
    if base.starts_with("http://") || base.starts_with("https://") {
        // Find the last slash in the path
        if let Some(last_slash) = base.rfind('/') {
            let base_dir = &base[..=last_slash];
            let combined = format!("{}{}", base_dir, relative);
            return Ok(normalize_url_path(&combined));
        }
    }

    // Fallback: just append
    Ok(format!("{}/{}", base, relative))
}

/// Normalizes a URL path by resolving `.` and `..` components.
fn normalize_url_path(url: &str) -> String {
    // Split URL into protocol+host and path
    let (prefix, path) = if let Some(pos) = url.find("://") {
        let after_protocol = &url[pos + 3..];
        if let Some(slash_pos) = after_protocol.find('/') {
            let host_end = pos + 3 + slash_pos;
            (&url[..host_end], &url[host_end..])
        } else {
            return url.to_string();
        }
    } else {
        return url.to_string();
    };

    // Normalize the path
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }

    format!("{}/{}", prefix, segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_to_safe_filename() {
        use std::collections::HashSet;
        let empty: HashSet<String> = HashSet::new();

        assert_eq!(
            uri_to_safe_filename("http://example.com/schemas/types.xsd", &empty),
            "types.xsd"
        );
        assert_eq!(
            uri_to_safe_filename("https://schemas.opengis.net/gml/3.2.1/gml.xsd", &empty),
            "gml.xsd"
        );
        assert_eq!(
            uri_to_safe_filename("file:///path/to/schema.xsd", &empty),
            "schema.xsd"
        );
    }

    #[test]
    fn test_uri_to_safe_filename_uniqueness() {
        use std::collections::HashSet;
        let mut existing: HashSet<String> = HashSet::new();
        existing.insert("types.xsd".to_string());

        // Should add hash suffix when filename already exists
        let filename = uri_to_safe_filename("http://example.com/other/types.xsd", &existing);
        assert!(filename.starts_with("types_"));
        assert!(filename.ends_with(".xsd"));
        assert_ne!(filename, "types.xsd");
    }

    #[test]
    fn test_extract_schema_locations() {
        let content = r#"
            <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
                <xs:import namespace="http://example.com" schemaLocation="types.xsd"/>
                <xs:include schemaLocation='common.xsd'/>
            </xs:schema>
        "#;
        let locations = extract_schema_locations(content);
        assert_eq!(locations.len(), 2);
        assert!(locations.contains(&"types.xsd".to_string()));
        assert!(locations.contains(&"common.xsd".to_string()));
    }

    #[test]
    fn test_extract_schema_locations_skips_xsi() {
        let content = r#"
            <root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                  xsi:schemaLocation="http://example.com http://example.com/schema.xsd">
            </root>
        "#;
        let locations = extract_schema_locations(content);
        // Should skip xsi:schemaLocation (contains space)
        assert!(locations.is_empty());
    }

    #[test]
    fn test_resolve_uri_absolute() {
        let result = resolve_uri("http://base.com/path/", "http://other.com/schema.xsd").unwrap();
        assert_eq!(result, "http://other.com/schema.xsd");
    }

    #[test]
    fn test_resolve_uri_relative_http() {
        let result = resolve_uri("http://example.com/schemas/main.xsd", "types.xsd").unwrap();
        assert_eq!(result, "http://example.com/schemas/types.xsd");
    }

    #[test]
    fn test_rewrite_schema_locations() {
        let content = br#"<xs:import schemaLocation="http://example.com/types.xsd"/>"#;
        let mut mapping = HashMap::new();
        mapping.insert(
            "http://example.com/types.xsd".to_string(),
            "types.xsd".to_string(),
        );

        let result =
            rewrite_schema_locations(content, "http://example.com/main.xsd", &mapping).unwrap();
        let result_str = std::str::from_utf8(&result).unwrap();

        assert!(result_str.contains(r#"schemaLocation="types.xsd""#));
    }

    #[test]
    fn test_rewrite_schema_locations_relative_path() {
        let content = br#"<xs:import schemaLocation="../types/common.xsd"/>"#;
        let mut mapping = HashMap::new();
        mapping.insert(
            "http://example.com/types/common.xsd".to_string(),
            "common.xsd".to_string(),
        );

        let result =
            rewrite_schema_locations(content, "http://example.com/schemas/main.xsd", &mapping)
                .unwrap();
        let result_str = std::str::from_utf8(&result).unwrap();

        assert!(result_str.contains(r#"schemaLocation="common.xsd""#));
    }
}
