//! Read-only view of an element reported by
//! [`Transformer::on_match`](super::Transformer::on_match).

use indexmap::IndexMap;

/// An element matched by an [`on_match`](super::Transformer::on_match)
/// handler: its name, namespace and attributes as written in its start tag.
///
/// Unlike the [`EditableNode`](super::EditableNode) passed to
/// [`on`](super::Transformer::on), it carries no children or text, so no
/// subtree is buffered to produce it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMatch {
    qname: String,
    /// Byte length of the prefix in `qname` (`0` when unprefixed).
    prefix_len: usize,
    namespace_uri: Option<String>,
    attributes: IndexMap<String, String>,
    /// Namespace URI of each attribute, in `attributes` order.
    attribute_namespaces: Vec<Option<String>>,
}

impl ElementMatch {
    /// Builds a match from the element's qualified name, its namespace URI,
    /// and its attributes in document order as `(qualified name, value,
    /// namespace URI)`. Namespace declarations must already be excluded.
    pub(crate) fn new(
        qname: String,
        namespace_uri: Option<String>,
        attributes: impl IntoIterator<Item = (String, String, Option<String>)>,
    ) -> Self {
        let prefix_len = qname.find(':').unwrap_or(0);
        let mut map = IndexMap::new();
        let mut attribute_namespaces = Vec::new();
        for (name, value, ns) in attributes {
            // A repeated name (not well-formed) keeps its first value, so
            // the two lists stay aligned.
            if let indexmap::map::Entry::Vacant(slot) = map.entry(name) {
                slot.insert(value);
                attribute_namespaces.push(ns);
            }
        }
        let attributes = map;
        Self {
            qname,
            prefix_len,
            namespace_uri,
            attributes,
            attribute_namespaces,
        }
    }

    /// The local name (without prefix).
    pub fn name(&self) -> &str {
        match self.prefix_len {
            0 => &self.qname,
            n => &self.qname[n + 1..],
        }
    }

    /// The qualified name as written (`prefix:local` or `local`).
    pub fn qname(&self) -> &str {
        &self.qname
    }

    /// The namespace prefix as written, if any.
    pub fn prefix(&self) -> Option<&str> {
        (self.prefix_len > 0).then(|| &self.qname[..self.prefix_len])
    }

    /// The namespace URI, resolved from the document's in-scope declarations.
    pub fn namespace_uri(&self) -> Option<&str> {
        self.namespace_uri.as_deref()
    }

    /// The value of the attribute with this qualified name, as written
    /// (e.g. `"id"` or `"xlink:href"`).
    pub fn get_attribute(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    /// The value of the attribute with this namespace URI and local name.
    pub fn get_attribute_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .zip(&self.attribute_namespaces)
            .find(|((name, _), ns)| {
                ns.as_deref() == Some(namespace_uri) && name.rsplit(':').next() == Some(local_name)
            })
            .map(|((_, value), _)| value.as_str())
    }

    /// All attributes, keyed by qualified name, in document order. Namespace
    /// declarations (`xmlns`, `xmlns:*`) are not included.
    pub fn get_attributes(&self) -> &IndexMap<String, String> {
        &self.attributes
    }
}
