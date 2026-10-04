//! Schema-location hints read from an instance document's root element.
//!
//! XSD 1.0 §4.3.2 defines two hint attributes in the
//! `http://www.w3.org/2001/XMLSchema-instance` namespace:
//! `schemaLocation` (whitespace-separated namespace/location pairs) and
//! `noNamespaceSchemaLocation` (one location for a schema without a target
//! namespace). Both are recognised by namespace URI, so any prefix bound to
//! that namespace works and an `xsi:` prefix bound to anything else does not.
//!
//! The auto-detecting validator ([`Validator`](crate::schema::Validator)
//! without `.schema(..)`) and the schema export/resolve functions all read
//! the hints through this module, so they agree on what a document asks for.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::document::XmlDocument;
use crate::error::Result;
use crate::namespace::Namespace;

/// The XML Schema instance namespace (`xsi:`).
pub(crate) const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// One schema document the instance asks to be validated against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchemaHint {
    /// Target namespace from `xsi:schemaLocation`; `None` for
    /// `xsi:noNamespaceSchemaLocation`.
    pub namespace: Option<String>,
    /// The schema location exactly as written in the document.
    pub location: String,
}

impl SchemaHint {
    /// Names the attribute this hint came from, for error messages.
    pub fn describe(&self) -> String {
        match &self.namespace {
            Some(ns) => format!("xsi:schemaLocation entry for namespace '{ns}'"),
            None => "xsi:noNamespaceSchemaLocation".to_string(),
        }
    }
}

/// All schema-location hints on a root element.
#[derive(Debug, Default, Clone)]
pub(crate) struct SchemaHints {
    /// Hints in document order (`xsi:schemaLocation` pairs first, then
    /// `xsi:noNamespaceSchemaLocation`).
    pub hints: Vec<SchemaHint>,
    /// Hint attributes that are present but malformed, as messages.
    pub problems: Vec<String>,
}

impl SchemaHints {
    /// True when the root element carries no hint attribute at all.
    pub fn is_empty(&self) -> bool {
        self.hints.is_empty() && self.problems.is_empty()
    }

    /// Builds the hints from `(namespace URI, local name, value)` triples.
    fn from_resolved<'a>(
        attrs: impl IntoIterator<Item = (Option<&'a str>, &'a str, &'a str)>,
    ) -> Self {
        let mut schema_location = None;
        let mut no_namespace = None;
        for (ns, local, value) in attrs {
            if ns != Some(XSI_NAMESPACE) {
                continue;
            }
            match local {
                "schemaLocation" => schema_location = Some(value),
                "noNamespaceSchemaLocation" => no_namespace = Some(value),
                _ => {}
            }
        }

        let mut out = Self::default();
        if let Some(value) = schema_location {
            let tokens: Vec<&str> = value.split_whitespace().collect();
            for pair in tokens.chunks(2) {
                match pair {
                    [ns, loc] => out.hints.push(SchemaHint {
                        namespace: Some((*ns).to_string()),
                        location: (*loc).to_string(),
                    }),
                    [ns] => out.problems.push(format!(
                        "xsi:schemaLocation must hold namespace/location pairs, \
                         but namespace '{ns}' has no location"
                    )),
                    _ => unreachable!("chunks(2) yields one or two tokens"),
                }
            }
            if tokens.is_empty() {
                out.problems
                    .push("xsi:schemaLocation is present but empty".to_string());
            }
        }
        if let Some(value) = no_namespace {
            let loc = value.trim();
            if loc.is_empty() {
                out.problems
                    .push("xsi:noNamespaceSchemaLocation is present but empty".to_string());
            } else {
                out.hints.push(SchemaHint {
                    namespace: None,
                    location: loc.to_string(),
                });
            }
        }
        out
    }

    /// Reads the hints from a streaming root start tag (raw qualified
    /// attribute names plus the namespace declarations on that tag).
    pub fn from_start_tag(attributes: &[(&str, Cow<'_, str>)], decls: &[Namespace]) -> Self {
        let resolve = |prefix: &str| {
            decls
                .iter()
                .rev()
                .find(|d| d.prefix() == prefix)
                .map(|d| d.uri())
        };
        Self::from_resolved(attributes.iter().filter_map(|(qname, value)| {
            let (prefix, local) = qname.split_once(':')?;
            Some((resolve(prefix), local, value.as_ref()))
        }))
    }

    /// Reads the hints from a parsed document's root element.
    pub fn from_document(doc: &XmlDocument) -> Result<Self> {
        let root = doc.get_root_element()?;
        let mut triples: Vec<(String, &'static str, String)> = Vec::new();
        for local in ["schemaLocation", "noNamespaceSchemaLocation"] {
            if let (Some((_, uri)), Some(value)) =
                (root.get_attribute_ns_info(local), root.get_attribute(local))
            {
                triples.push((uri, local, value));
            }
        }
        Ok(Self::from_resolved(triples.iter().map(
            |(uri, local, value)| (Some(uri.as_str()), *local, value.as_str()),
        )))
    }

    /// Reads the hints from serialized XML, stopping at the root start tag
    /// (no DOM is built).
    pub fn from_xml_bytes(xml: &[u8]) -> Result<Self> {
        use quick_xml::Reader;
        use quick_xml::events::Event;

        let mut reader = Reader::from_reader(xml);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(e) | Event::Empty(e) => {
                    let mut decls: HashMap<String, String> = HashMap::new();
                    let mut attrs: Vec<(String, String)> = Vec::new();
                    for attr in e.attributes() {
                        let attr = attr?;
                        let key = std::str::from_utf8(attr.key.as_ref())?.to_string();
                        let value = attr.unescape_value()?.into_owned();
                        if let Some(prefix) = key.strip_prefix("xmlns:") {
                            decls.insert(prefix.to_string(), value);
                        } else {
                            attrs.push((key, value));
                        }
                    }
                    return Ok(Self::from_resolved(attrs.iter().filter_map(
                        |(qname, value)| {
                            let (prefix, local) = qname.split_once(':')?;
                            Some((decls.get(prefix).map(String::as_str), local, value.as_str()))
                        },
                    )));
                }
                Event::Eof => return Ok(Self::default()),
                _ => {}
            }
            buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(xml: &str) -> SchemaHints {
        SchemaHints::from_xml_bytes(xml.as_bytes()).unwrap()
    }

    #[test]
    fn reads_pairs_and_no_namespace_hint() {
        let h = bytes(
            r#"<r xmlns:q="http://www.w3.org/2001/XMLSchema-instance"
                  q:schemaLocation="urn:a a.xsd  urn:b b.xsd" q:noNamespaceSchemaLocation=" c.xsd "/>"#,
        );
        assert!(h.problems.is_empty());
        let got: Vec<(Option<&str>, &str)> = h
            .hints
            .iter()
            .map(|h| (h.namespace.as_deref(), h.location.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("urn:a"), "a.xsd"),
                (Some("urn:b"), "b.xsd"),
                (None, "c.xsd")
            ]
        );
    }

    #[test]
    fn ignores_attributes_outside_the_xsi_namespace() {
        assert!(bytes(r#"<r schemaLocation="urn:a a.xsd"/>"#).is_empty());
        assert!(bytes(r#"<r xmlns:xsi="urn:x" xsi:schemaLocation="urn:a a.xsd"/>"#).is_empty());
    }

    #[test]
    fn reports_dangling_namespace() {
        let h = bytes(
            r#"<r xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:a a.xsd urn:b"/>"#,
        );
        assert_eq!(h.hints.len(), 1);
        assert_eq!(h.problems.len(), 1);
        assert!(h.problems[0].contains("urn:b"));
    }

    #[test]
    fn document_and_bytes_agree() {
        let xml = r#"<r xmlns:i="http://www.w3.org/2001/XMLSchema-instance" i:noNamespaceSchemaLocation="x.xsd"/>"#;
        let doc = crate::parse(xml).unwrap();
        let from_doc = SchemaHints::from_document(&doc).unwrap();
        assert_eq!(from_doc.hints, bytes(xml).hints);
    }
}
