//! Namespace handling for XML documents.

pub mod error;

use std::collections::HashMap;

/// Represents an XML namespace with prefix and URI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Namespace {
    /// Namespace prefix (e.g., "gml", "uro"). Empty string for default namespace.
    prefix: String,
    /// Namespace URI (e.g., `http://www.opengis.net/gml`)
    uri: String,
}

impl Namespace {
    /// Creates a new namespace with prefix and URI.
    pub fn new(prefix: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            uri: uri.into(),
        }
    }

    /// Creates a default namespace (no prefix).
    pub fn default_ns(uri: impl Into<String>) -> Self {
        Self {
            prefix: String::new(),
            uri: uri.into(),
        }
    }

    /// Returns the namespace prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Returns the namespace URI (href).
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Alias for uri() to match libxml API.
    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    /// Alias for uri() to match libxml API.
    pub fn get_href(&self) -> &str {
        &self.uri
    }

    /// Returns true if this is the default namespace.
    pub fn is_default(&self) -> bool {
        self.prefix.is_empty()
    }
}

/// Manages namespace bindings for XPath evaluation.
#[derive(Debug, Clone, Default)]
pub struct NamespaceResolver {
    /// Maps prefix to URI
    prefix_to_uri: HashMap<String, String>,
    /// Maps URI to prefix (for reverse lookups)
    uri_to_prefix: HashMap<String, String>,
}

impl NamespaceResolver {
    /// Creates a new empty namespace resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a namespace binding.
    pub fn register(&mut self, prefix: &str, uri: &str) {
        self.prefix_to_uri
            .insert(prefix.to_string(), uri.to_string());
        self.uri_to_prefix
            .insert(uri.to_string(), prefix.to_string());
    }

    /// Resolves a prefix to its namespace URI.
    pub fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        self.prefix_to_uri.get(prefix).map(|s| s.as_str())
    }

    /// Resolves a URI to its prefix.
    pub fn resolve_uri(&self, uri: &str) -> Option<&str> {
        self.uri_to_prefix.get(uri).map(|s| s.as_str())
    }

    /// Returns all registered namespace bindings as (prefix, uri) pairs.
    pub fn bindings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.prefix_to_uri
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Returns the number of registered namespaces.
    pub fn len(&self) -> usize {
        self.prefix_to_uri.len()
    }

    /// Returns true if no namespaces are registered.
    pub fn is_empty(&self) -> bool {
        self.prefix_to_uri.is_empty()
    }
}

/// Common XML namespace URIs.
pub mod common {
    /// XML namespace for XML attributes like xml:lang
    pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

    /// XML Schema instance namespace
    pub const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

    /// XML Schema namespace
    pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
}

/// Splits a qualified name into prefix and local name.
///
/// # Examples
/// ```
/// use fastxml::namespace::split_qname;
///
/// assert_eq!(split_qname("gml:name"), (Some("gml"), "name"));
/// assert_eq!(split_qname("Building"), (None, "Building"));
/// ```
pub fn split_qname(qname: &str) -> (Option<&str>, &str) {
    match qname.split_once(':') {
        Some((prefix, local)) => (Some(prefix), local),
        None => (None, qname),
    }
}

/// Joins a prefix and local name into a qualified name.
///
/// # Examples
/// ```
/// use fastxml::namespace::join_qname;
///
/// assert_eq!(join_qname(Some("gml"), "name"), "gml:name");
/// assert_eq!(join_qname(None, "Building"), "Building");
/// ```
pub fn join_qname(prefix: Option<&str>, local_name: &str) -> String {
    match prefix {
        Some(p) if !p.is_empty() => format!("{}:{}", p, local_name),
        _ => local_name.to_string(),
    }
}

/// Extracts namespace declarations from the first element without full DOM parsing.
///
/// This is a lightweight operation that reads only the first element's attributes
/// to extract xmlns declarations. Useful for registering namespaces before
/// streaming transformation.
///
/// # Returns
///
/// A map of prefix -> URI. Empty string key represents the default namespace.
///
/// # Example
///
/// ```rust
/// use fastxml::namespace::extract_root_namespaces;
///
/// let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns="http://example.com">
///     <gml:point/>
/// </root>"#;
///
/// let namespaces = extract_root_namespaces(xml).unwrap();
/// assert_eq!(namespaces.get("gml"), Some(&"http://www.opengis.net/gml".to_string()));
/// assert_eq!(namespaces.get(""), Some(&"http://example.com".to_string()));
/// ```
pub fn extract_root_namespaces(xml: &str) -> Result<HashMap<String, String>, crate::error::Error> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let mut namespaces = HashMap::new();
                for attr in e.attributes().filter_map(|a| a.ok()) {
                    let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
                        crate::error::Error::Parse(crate::parser::error::ParseError::Generic {
                            message: e.to_string(),
                        })
                    })?;
                    let value = attr.unescape_value().map_err(|e| {
                        crate::error::Error::Parse(crate::parser::error::ParseError::Generic {
                            message: e.to_string(),
                        })
                    })?;

                    if let Some(prefix) = key.strip_prefix("xmlns:") {
                        namespaces.insert(prefix.to_string(), value.to_string());
                    } else if key == "xmlns" {
                        namespaces.insert(String::new(), value.to_string());
                    }
                }
                return Ok(namespaces);
            }
            Ok(Event::Eof) => return Ok(HashMap::new()),
            Ok(_) => {
                buf.clear();
                continue;
            }
            Err(e) => {
                return Err(crate::error::Error::Parse(
                    crate::parser::error::ParseError::Generic {
                        message: e.to_string(),
                    },
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_creation() {
        let ns = Namespace::new("gml", "http://www.opengis.net/gml");
        assert_eq!(ns.prefix(), "gml");
        assert_eq!(ns.uri(), "http://www.opengis.net/gml");
        assert!(!ns.is_default());

        let default_ns = Namespace::default_ns("http://example.com");
        assert_eq!(default_ns.prefix(), "");
        assert!(default_ns.is_default());
    }

    #[test]
    fn test_namespace_resolver() {
        let mut resolver = NamespaceResolver::new();
        resolver.register("gml", "http://www.opengis.net/gml");
        resolver.register("uro", "https://www.geospatial.jp/iur/uro/3.0");

        assert_eq!(
            resolver.resolve_prefix("gml"),
            Some("http://www.opengis.net/gml")
        );
        assert_eq!(
            resolver.resolve_uri("https://www.geospatial.jp/iur/uro/3.0"),
            Some("uro")
        );
        assert_eq!(resolver.resolve_prefix("unknown"), None);
        assert_eq!(resolver.len(), 2);
    }

    #[test]
    fn test_split_qname() {
        assert_eq!(split_qname("gml:name"), (Some("gml"), "name"));
        assert_eq!(split_qname("Building"), (None, "Building"));
        assert_eq!(split_qname("ns:local:extra"), (Some("ns"), "local:extra"));
    }

    #[test]
    fn test_join_qname() {
        assert_eq!(join_qname(Some("gml"), "name"), "gml:name");
        assert_eq!(join_qname(None, "Building"), "Building");
        assert_eq!(join_qname(Some(""), "Building"), "Building");
    }

    #[test]
    fn test_extract_root_namespaces_with_prefixes() {
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml" xmlns:uro="http://example.com/uro">
            <gml:point/>
        </root>"#;

        let ns = extract_root_namespaces(xml).unwrap();
        assert_eq!(
            ns.get("gml"),
            Some(&"http://www.opengis.net/gml".to_string())
        );
        assert_eq!(ns.get("uro"), Some(&"http://example.com/uro".to_string()));
        assert_eq!(ns.len(), 2);
    }

    #[test]
    fn test_extract_root_namespaces_with_default() {
        let xml = r#"<root xmlns="http://example.com/default" xmlns:ns="http://example.com/ns">
            <element/>
        </root>"#;

        let ns = extract_root_namespaces(xml).unwrap();
        assert_eq!(ns.get(""), Some(&"http://example.com/default".to_string()));
        assert_eq!(ns.get("ns"), Some(&"http://example.com/ns".to_string()));
    }

    #[test]
    fn test_extract_root_namespaces_empty_element() {
        let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"/>"#;

        let ns = extract_root_namespaces(xml).unwrap();
        assert_eq!(
            ns.get("gml"),
            Some(&"http://www.opengis.net/gml".to_string())
        );
    }

    #[test]
    fn test_extract_root_namespaces_no_namespaces() {
        let xml = r#"<root id="1"><element/></root>"#;

        let ns = extract_root_namespaces(xml).unwrap();
        assert!(ns.is_empty());
    }

    #[test]
    fn test_extract_root_namespaces_empty_xml() {
        let xml = "";

        let ns = extract_root_namespaces(xml).unwrap();
        assert!(ns.is_empty());
    }
}
