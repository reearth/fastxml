//! Schema export utilities.
//!
//! This module fetches the schemas an instance document names (and
//! everything they import, include or redefine) and writes them to a local
//! directory with every `schemaLocation` rewritten to the exported file
//! name, plus an OASIS `catalog.xml` mapping the original URIs to the files.
//!
//! This is useful for tools like libxml that need all schemas in a single
//! directory with relative paths.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use indexmap::IndexMap;

use crate::error::{Error, Result};
use crate::schema::error::SchemaError;
use crate::schema::fetcher::SchemaFetcher;
use crate::schema::hints::SchemaHints;

/// Result of schema export operation.
#[derive(Debug, Clone)]
pub struct ExportResult {
    /// Number of schema documents exported (one file each).
    pub schema_count: usize,
    /// Map of original URIs to local filenames. A document reached under
    /// more than one URI (e.g. before and after an HTTP redirect) has one
    /// entry per URI, all naming the same file.
    pub uri_to_filename: HashMap<String, String>,
    /// The entry schema filename (the first schema named by the document).
    pub entry_filename: Option<String>,
}

/// Exports schemas from xsi:schemaLocation to a local directory.
///
/// This function:
/// 1. Reads the root element's `xsi:schemaLocation` and
///    `xsi:noNamespaceSchemaLocation` hints
/// 2. Fetches all referenced schemas (including imports/includes/redefines)
/// 3. Rewrites every import/include/redefine `schemaLocation` to the local
///    file name of the document it resolves to
/// 4. Writes all schemas, byte for byte apart from those rewrites, plus a
///    `catalog.xml` to the output directory
///
/// Files are written into `output_dir` (created if missing), replacing files
/// with the same names; nothing else in the directory is touched. Each
/// document gets a file name that is unique ignoring case, so the set is
/// safe on case-insensitive filesystems. Names are assigned in discovery
/// order, so the same inputs always produce the same files.
///
/// # Errors
///
/// Any referenced schema that cannot be fetched or parsed is an error (the
/// exported set would otherwise silently point outside itself), as is a
/// malformed hint attribute. A document without hints exports nothing and
/// returns `schema_count == 0`.
///
/// # Arguments
///
/// * `xml_content` - The XML document content
/// * `output_dir` - Directory to write schemas to
/// * `fetcher` - Schema fetcher for downloading schemas; relative hint
///   locations are resolved by the fetcher
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
    let hints = SchemaHints::from_xml_bytes(xml_content)?;
    if hints.is_empty() {
        return Ok(ExportResult {
            schema_count: 0,
            uri_to_filename: HashMap::new(),
            entry_filename: None,
        });
    }
    let locations = hint_locations(&hints, |loc| loc.to_string())?;
    let set = SchemaSet::collect(&locations, fetcher)?;
    set.write_to(output_dir)?;
    Ok(set.result())
}

/// The hint locations, mapped through `resolve`; a malformed hint is an error.
pub(crate) fn hint_locations(
    hints: &SchemaHints,
    resolve: impl Fn(&str) -> String,
) -> Result<Vec<String>> {
    if let Some(problem) = hints.problems.first() {
        return Err(Error::InvalidOperation(format!(
            "malformed schema-location hint: {problem}"
        )));
    }
    Ok(hints.hints.iter().map(|h| resolve(&h.location)).collect())
}

/// One fetched schema document.
struct Doc {
    /// The URI the document was fetched as (after redirects); relative
    /// locations inside it resolve against this.
    uri: String,
    content: Vec<u8>,
    filename: String,
}

/// A closed set of schema documents: the hinted entries and everything they
/// reference, in discovery order (entries first, then breadth-first).
pub(crate) struct SchemaSet {
    docs: Vec<Doc>,
    /// Every URI a reference resolved to → index into `docs`.
    by_uri: IndexMap<String, usize>,
}

impl SchemaSet {
    /// Fetches the entry `locations` and, transitively, every document they
    /// import, include or redefine. Any fetch or parse failure is an error.
    pub(crate) fn collect<F: SchemaFetcher>(locations: &[String], fetcher: &F) -> Result<Self> {
        let mut set = Self {
            docs: Vec::new(),
            by_uri: IndexMap::new(),
        };
        let mut queue = VecDeque::new();
        for location in locations {
            if let Some(idx) = set.fetch_new(location, fetcher)? {
                queue.push_back(idx);
            }
        }
        while let Some(idx) = queue.pop_front() {
            let base = set.docs[idx].uri.clone();
            for location in referenced_locations(&set.docs[idx].content, &base)? {
                let resolved = resolve_uri(&base, &location)?;
                if let Some(new_idx) = set.fetch_new(&resolved, fetcher)? {
                    queue.push_back(new_idx);
                }
            }
        }
        set.assign_filenames();
        Ok(set)
    }

    /// Fetches `uri` unless it is already known; returns the index of a
    /// newly added document.
    fn fetch_new<F: SchemaFetcher>(&mut self, uri: &str, fetcher: &F) -> Result<Option<usize>> {
        if self.by_uri.contains_key(uri) {
            return Ok(None);
        }
        let fetched = fetcher.fetch(uri)?;
        if let Some(&idx) = self.by_uri.get(&fetched.final_url) {
            // Another URI for a document we already have (a redirect).
            self.by_uri.insert(uri.to_string(), idx);
            return Ok(None);
        }
        let idx = self.docs.len();
        self.by_uri.insert(fetched.final_url.clone(), idx);
        self.by_uri.insert(uri.to_string(), idx);
        self.docs.push(Doc {
            uri: fetched.final_url,
            content: fetched.content,
            filename: String::new(),
        });
        Ok(Some(idx))
    }

    fn assign_filenames(&mut self) {
        let mut taken: HashSet<String> = HashSet::new();
        for doc in &mut self.docs {
            doc.filename = uri_to_safe_filename(&doc.uri, &taken);
            taken.insert(doc.filename.to_lowercase());
        }
    }

    /// `(uri, content as UTF-8)` pairs in discovery order, for compilation.
    pub(crate) fn sources(&self) -> Vec<(&str, std::borrow::Cow<'_, [u8]>)> {
        self.docs
            .iter()
            .map(|d| (d.uri.as_str(), crate::parser::encoding::to_utf8(&d.content)))
            .collect()
    }

    /// Writes every document (with rewritten locations) and `catalog.xml`.
    pub(crate) fn write_to(&self, output_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(output_dir)?;
        for doc in &self.docs {
            let rewritten = rewrite_schema_locations(&doc.content, |location| {
                let resolved = resolve_uri(&doc.uri, location).ok()?;
                let idx = *self.by_uri.get(&resolved)?;
                Some(self.docs[idx].filename.clone())
            });
            std::fs::write(output_dir.join(&doc.filename), rewritten)?;
        }
        // Also write an OASIS XML catalog mapping the original URIs to the
        // exported files, so catalog-aware tools (libxml2/xmllint via
        // XML_CATALOG_FILES) can resolve the unmodified schema URLs offline —
        // including URLs that are no longer fetchable by tools without
        // redirect/TLS support.
        write_catalog(output_dir, &self.uri_to_filename())
    }

    fn uri_to_filename(&self) -> HashMap<String, String> {
        self.by_uri
            .iter()
            .map(|(uri, &idx)| (uri.clone(), self.docs[idx].filename.clone()))
            .collect()
    }

    pub(crate) fn entry_filename(&self) -> Option<String> {
        self.docs.first().map(|d| d.filename.clone())
    }

    pub(crate) fn result(&self) -> ExportResult {
        ExportResult {
            schema_count: self.docs.len(),
            uri_to_filename: self.uri_to_filename(),
            entry_filename: self.entry_filename(),
        }
    }
}

/// The import/include/redefine locations a schema document references.
fn referenced_locations(content: &[u8], uri: &str) -> Result<Vec<String>> {
    let content = crate::parser::encoding::to_utf8(content);
    let schema = crate::schema::xsd::parse_xsd_ast(&content).map_err(|e| {
        Error::from(SchemaError::InvalidSchema {
            message: format!("{uri}: {e}"),
        })
    })?;
    let mut locations: Vec<String> = schema
        .imports
        .iter()
        .filter_map(|i| i.schema_location.clone())
        .collect();
    locations.extend(schema.includes.iter().map(|i| i.schema_location.clone()));
    locations.extend(schema.redefines.iter().map(|r| r.schema_location.clone()));
    Ok(locations)
}

/// Writes `catalog.xml` (OASIS XML Catalogs format) into the export
/// directory, mapping each original schema URI to its local filename.
fn write_catalog(output_dir: &Path, uri_to_filename: &HashMap<String, String>) -> Result<()> {
    let mut entries: Vec<(&String, &String)> = uri_to_filename.iter().collect();
    entries.sort();

    let mut catalog = String::from(
        "<?xml version=\"1.0\"?>\n<catalog xmlns=\"urn:oasis:names:tc:entity:xmlns:xml:catalog\">\n",
    );
    for (uri, filename) in entries {
        let uri = xml_escape_attr(uri);
        let filename = xml_escape_attr(filename);
        // <uri> covers namespace/schemaLocation lookups; <system> covers
        // system-identifier lookups. Emit both so any resolver path hits.
        catalog.push_str(&format!("  <uri name=\"{uri}\" uri=\"{filename}\"/>\n"));
        catalog.push_str(&format!(
            "  <system systemId=\"{uri}\" uri=\"{filename}\"/>\n"
        ));
    }
    catalog.push_str("</catalog>\n");

    std::fs::write(output_dir.join("catalog.xml"), catalog)?;
    Ok(())
}

fn xml_escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// Rewrites the value of every `schemaLocation` attribute for which
/// `target` returns a replacement, leaving all other bytes untouched.
///
/// Works on raw bytes, so any ASCII-compatible encoding (UTF-8,
/// ISO-8859-*, Shift_JIS, …) round-trips exactly; a UTF-16 document is
/// written unchanged. Only attributes whose name is exactly
/// `schemaLocation` (not `xsi:schemaLocation`) are touched.
fn rewrite_schema_locations(
    content: &[u8],
    mut target: impl FnMut(&str) -> Option<String>,
) -> Vec<u8> {
    const NAME: &[u8] = b"schemaLocation";
    let mut out = Vec::with_capacity(content.len());
    let mut copied = 0;
    let mut i = 0;
    while let Some(off) = memchr::memmem::find(&content[i..], NAME) {
        let start = i + off;
        i = start + NAME.len();
        if start == 0 || !content[start - 1].is_ascii_whitespace() {
            continue;
        }
        let Some((value_start, value_end)) = attribute_value_span(content, i) else {
            continue;
        };
        i = value_end + 1;
        let Ok(raw) = std::str::from_utf8(&content[value_start..value_end]) else {
            continue;
        };
        let Ok(value) = quick_xml::escape::unescape(raw) else {
            continue;
        };
        if let Some(filename) = target(value.trim()) {
            out.extend_from_slice(&content[copied..value_start]);
            out.extend_from_slice(filename.as_bytes());
            copied = value_end;
        }
    }
    out.extend_from_slice(&content[copied..]);
    out
}

/// For `content[pos..]` = `  =  "value"`, the byte span of `value`.
fn attribute_value_span(content: &[u8], mut pos: usize) -> Option<(usize, usize)> {
    let skip_ws = |mut p: usize| {
        while p < content.len() && content[p].is_ascii_whitespace() {
            p += 1;
        }
        p
    };
    pos = skip_ws(pos);
    if content.get(pos) != Some(&b'=') {
        return None;
    }
    pos = skip_ws(pos + 1);
    let quote = *content.get(pos).filter(|q| **q == b'"' || **q == b'\'')?;
    let start = pos + 1;
    let len = memchr::memchr(quote, &content[start..])?;
    Some((start, start + len))
}

/// Converts a URI to a safe filename that is unique among `taken`
/// (lower-cased names), so exports survive case-insensitive filesystems.
fn uri_to_safe_filename(uri: &str, taken: &HashSet<String>) -> String {
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

    if !taken.contains(&base_filename.to_lowercase()) {
        return base_filename;
    }

    // Add hash suffix to make it unique (and a counter in the unlikely case
    // the hashed name is taken too).
    let stem = base_filename.strip_suffix(".xsd").unwrap_or(&base_filename);
    let hash_suffix = format!("{:08x}", hash_uri(uri) as u32);
    let mut candidate = format!("{}_{}.xsd", stem, hash_suffix);
    let mut n = 1;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("{}_{}_{}.xsd", stem, hash_suffix, n);
        n += 1;
    }
    candidate
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
        let mut existing: HashSet<String> = HashSet::new();
        existing.insert("types.xsd".to_string());

        // Should add hash suffix when filename already exists
        let filename = uri_to_safe_filename("http://example.com/other/types.xsd", &existing);
        assert!(filename.starts_with("types_"));
        assert!(filename.ends_with(".xsd"));
        assert_ne!(filename, "types.xsd");

        // ... also when it differs only in case
        let filename = uri_to_safe_filename("http://example.com/other/Types.xsd", &existing);
        assert_ne!(filename.to_lowercase(), "types.xsd");
    }

    #[test]
    fn test_referenced_locations() {
        let content = br#"
            <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
                <!-- <xs:include schemaLocation="commented-out.xsd"/> -->
                <xs:import namespace="http://example.com" schemaLocation="types.xsd"/>
                <xs:include schemaLocation='common.xsd'/>
            </xs:schema>
        "#;
        let locations = referenced_locations(content, "file:///x.xsd").unwrap();
        assert_eq!(locations, vec!["types.xsd", "common.xsd"]);
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

    fn rewrite_with(content: &[u8], base: &str, map: &[(&str, &str)]) -> String {
        let map: HashMap<String, String> = map
            .iter()
            .map(|(u, f)| (u.to_string(), f.to_string()))
            .collect();
        let out = rewrite_schema_locations(content, |loc| {
            map.get(&resolve_uri(base, loc).ok()?).cloned()
        });
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_rewrite_schema_locations() {
        let result = rewrite_with(
            br#"<xs:import schemaLocation="http://example.com/types.xsd"/>"#,
            "http://example.com/main.xsd",
            &[("http://example.com/types.xsd", "types.xsd")],
        );
        assert_eq!(result, r#"<xs:import schemaLocation="types.xsd"/>"#);
    }

    #[test]
    fn test_rewrite_schema_locations_relative_path() {
        let result = rewrite_with(
            br#"<xs:import schemaLocation='../types/common.xsd'/>"#,
            "http://example.com/schemas/main.xsd",
            &[("http://example.com/types/common.xsd", "common.xsd")],
        );
        assert_eq!(result, r#"<xs:import schemaLocation='common.xsd'/>"#);
    }

    #[test]
    fn test_rewrite_resolves_bare_names_too() {
        // A bare name is not assumed to be rewritten already: it resolves
        // against the document and is mapped to that target's file.
        let result = rewrite_with(
            br#"<xs:include schemaLocation = "types.xsd"/>"#,
            "http://example.com/a/main.xsd",
            &[("http://example.com/a/types.xsd", "types_1234.xsd")],
        );
        assert_eq!(result, r#"<xs:include schemaLocation = "types_1234.xsd"/>"#);
    }

    #[test]
    fn test_rewrite_leaves_xsi_schema_location_alone() {
        let content = br#"<x xsi:schemaLocation="urn:a http://example.com/types.xsd"/>"#;
        let result = rewrite_with(
            content,
            "http://example.com/main.xsd",
            &[("http://example.com/types.xsd", "types.xsd")],
        );
        assert_eq!(result.as_bytes(), content);
    }
}
