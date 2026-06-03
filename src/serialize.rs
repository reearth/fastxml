//! XML serialization (node to string).

use std::io::Write;

use crate::document::XmlDocument;
use crate::error::Result;
use crate::node::{NodeType, XmlNode, XmlRoNode};

/// Options for XML serialization.
#[derive(Debug, Clone)]
pub struct SerializeOptions {
    /// Whether to add indentation.
    pub indent: bool,
    /// Indentation string (default: 2 spaces).
    pub indent_str: String,
    /// Whether to include XML declaration.
    pub xml_declaration: bool,
    /// Encoding for XML declaration.
    pub encoding: String,
}

impl Default for SerializeOptions {
    fn default() -> Self {
        Self {
            indent: false,
            indent_str: "  ".to_string(),
            xml_declaration: false,
            encoding: "UTF-8".to_string(),
        }
    }
}

impl SerializeOptions {
    /// Creates options with pretty-printing enabled.
    pub fn pretty() -> Self {
        Self {
            indent: true,
            ..Default::default()
        }
    }
}

/// Serializes a node to an XML string.
pub fn node_to_xml_string(doc: &XmlDocument, node: &XmlNode) -> Result<String> {
    node_to_xml_string_with_options(doc, node, &SerializeOptions::default())
}

/// Serializes a read-only node to an XML string.
pub fn readonly_node_to_xml_string(doc: &XmlDocument, node: &XmlRoNode) -> Result<String> {
    node_to_xml_string_with_options(doc, &node.clone().into_node(), &SerializeOptions::default())
}

/// Serializes a node with custom options.
pub fn node_to_xml_string_with_options(
    _doc: &XmlDocument,
    node: &XmlNode,
    options: &SerializeOptions,
) -> Result<String> {
    let mut output = Vec::new();
    let mut serializer = XmlSerializer::new(&mut output, options.clone());

    if options.xml_declaration {
        serializer.write_declaration()?;
    }

    serializer.write_node(node, 0)?;

    Ok(String::from_utf8(output)?)
}

/// Serializes an entire document to an XML string.
pub fn document_to_xml_string(doc: &XmlDocument) -> Result<String> {
    document_to_xml_string_with_options(doc, &SerializeOptions::default())
}

/// Serializes a document with custom options.
pub fn document_to_xml_string_with_options(
    doc: &XmlDocument,
    options: &SerializeOptions,
) -> Result<String> {
    let root = doc.get_root_element()?;
    let mut opts = options.clone();
    opts.xml_declaration = true;
    node_to_xml_string_with_options(doc, &root, &opts)
}

/// What a [`Printer`] serializes.
enum Target<'a> {
    /// An entire document (serialized from its root element).
    Document(&'a XmlDocument),
    /// A single mutable node.
    Node(&'a XmlNode),
    /// A single read-only node.
    RoNode(&'a XmlRoNode),
}

/// A consistent front door for XML serialization.
///
/// `Printer::from(source)` → optional formatting setters → a terminal
/// (`to_string` / `into_bytes` / `write_to`). The input type selects what is
/// serialized: a whole [`XmlDocument`] (from its root, with an XML declaration
/// by default), or a single [`XmlNode`] / [`XmlRoNode`] (no declaration by
/// default).
///
/// # Example
///
/// ```
/// use fastxml::{Parser, Printer};
///
/// let doc = Parser::from("<root><child>hi</child></root>").parse().unwrap();
///
/// // Whole document, pretty-printed.
/// let xml = Printer::from(&doc).pretty().to_string().unwrap();
/// assert!(xml.contains("<child>hi</child>"));
/// ```
pub struct Printer<'a> {
    target: Target<'a>,
    options: SerializeOptions,
}

impl<'a> From<&'a XmlDocument> for Printer<'a> {
    fn from(doc: &'a XmlDocument) -> Self {
        // Whole-document output includes the XML declaration by default,
        // matching `document_to_xml_string`.
        Self {
            target: Target::Document(doc),
            options: SerializeOptions {
                xml_declaration: true,
                ..SerializeOptions::default()
            },
        }
    }
}

impl<'a> From<&'a XmlNode> for Printer<'a> {
    fn from(node: &'a XmlNode) -> Self {
        Self {
            target: Target::Node(node),
            options: SerializeOptions::default(),
        }
    }
}

impl<'a> From<&'a XmlRoNode> for Printer<'a> {
    fn from(node: &'a XmlRoNode) -> Self {
        Self {
            target: Target::RoNode(node),
            options: SerializeOptions::default(),
        }
    }
}

impl<'a> Printer<'a> {
    /// Enables indentation (pretty-printing) with the default 2-space indent.
    pub fn pretty(mut self) -> Self {
        self.options.indent = true;
        self
    }

    /// Enables indentation using `indent` as the per-level string.
    pub fn indent(mut self, indent: impl Into<String>) -> Self {
        self.options.indent = true;
        self.options.indent_str = indent.into();
        self
    }

    /// Controls whether an `<?xml ... ?>` declaration is emitted.
    ///
    /// Defaults to `true` for a document and `false` for a single node.
    pub fn declaration(mut self, yes: bool) -> Self {
        self.options.xml_declaration = yes;
        self
    }

    /// Sets the encoding written in the XML declaration (default `UTF-8`).
    pub fn encoding(mut self, encoding: impl Into<String>) -> Self {
        self.options.encoding = encoding.into();
        self
    }

    /// Serializes to the given writer.
    pub fn write_to<W: Write>(self, writer: &mut W) -> Result<()> {
        let owned: Option<XmlNode> = match &self.target {
            Target::Document(doc) => Some(doc.get_root_element()?),
            Target::RoNode(node) => Some((*node).clone().into_node()),
            Target::Node(_) => None,
        };
        let node = match &self.target {
            Target::Node(node) => *node,
            _ => owned
                .as_ref()
                .expect("owned node present for document/ro-node"),
        };

        let mut serializer = XmlSerializer::new(writer, self.options.clone());
        if self.options.xml_declaration {
            serializer.write_declaration()?;
        }
        serializer.write_node(node, 0)?;
        Ok(())
    }

    /// Serializes to a byte vector.
    pub fn into_bytes(self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.write_to(&mut buf)?;
        Ok(buf)
    }

    /// Serializes to a `String`.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_string(self) -> Result<String> {
        Ok(String::from_utf8(self.into_bytes()?)?)
    }
}

/// XML serializer.
struct XmlSerializer<W: Write> {
    writer: W,
    options: SerializeOptions,
}

impl<W: Write> XmlSerializer<W> {
    fn new(writer: W, options: SerializeOptions) -> Self {
        Self { writer, options }
    }

    fn write_declaration(&mut self) -> Result<()> {
        write!(
            self.writer,
            "<?xml version=\"1.0\" encoding=\"{}\"?>",
            self.options.encoding
        )?;
        if self.options.indent {
            writeln!(self.writer)?;
        }
        Ok(())
    }

    fn write_node(&mut self, node: &XmlNode, depth: usize) -> Result<()> {
        match node.get_type() {
            NodeType::Document => {
                for child in node.get_child_nodes() {
                    self.write_node(&child, depth)?;
                }
            }
            NodeType::Element => {
                self.write_element(node, depth)?;
            }
            NodeType::Text => {
                if let Some(content) = node.get_content() {
                    self.write_escaped_text(&content)?;
                }
            }
            NodeType::CData => {
                if let Some(content) = node.get_content() {
                    write!(self.writer, "<![CDATA[{}]]>", content)?;
                }
            }
            NodeType::Comment => {
                if let Some(content) = node.get_content() {
                    write!(self.writer, "<!--{}-->", content)?;
                }
            }
            NodeType::ProcessingInstruction => {
                let name = node.get_name();
                if let Some(content) = node.get_content() {
                    write!(self.writer, "<?{} {}?>", name, content)?;
                } else {
                    write!(self.writer, "<?{}?>", name)?;
                }
            }
            NodeType::Attribute => {
                // Attributes are handled in write_element
            }
            NodeType::Namespace => {
                // Namespace nodes are virtual (for XPath), not serialized
            }
        }
        Ok(())
    }

    fn write_element(&mut self, node: &XmlNode, depth: usize) -> Result<()> {
        let qname = node.qname();
        let attributes = node.get_attributes();
        let namespace_decls = node.get_namespace_declarations();
        let children = node.get_child_nodes();

        // Indentation
        if self.options.indent && depth > 0 {
            for _ in 0..depth {
                write!(self.writer, "{}", self.options.indent_str)?;
            }
        }

        // Start tag
        write!(self.writer, "<{}", qname)?;

        // Namespace declarations
        for ns in &namespace_decls {
            if ns.prefix().is_empty() {
                write!(self.writer, " xmlns=\"{}\"", escape_attribute(ns.uri()))?;
            } else {
                write!(
                    self.writer,
                    " xmlns:{}=\"{}\"",
                    ns.prefix(),
                    escape_attribute(ns.uri())
                )?;
            }
        }

        // Attributes
        for (name, value) in &attributes {
            let attr_qname = if let Some((prefix, _uri)) = node.get_attribute_ns_info(name) {
                if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{}:{}", prefix, name)
                }
            } else {
                name.clone()
            };
            write!(
                self.writer,
                " {}=\"{}\"",
                attr_qname,
                escape_attribute(value)
            )?;
        }

        if children.is_empty() {
            // Self-closing tag
            write!(self.writer, "/>")?;
        } else {
            write!(self.writer, ">")?;

            // Check if we should add newlines
            let has_element_children = children.iter().any(|c| c.is_element());

            if self.options.indent && has_element_children {
                writeln!(self.writer)?;
            }

            // Children
            for child in &children {
                self.write_node(child, depth + 1)?;
                if self.options.indent && child.is_element() {
                    writeln!(self.writer)?;
                }
            }

            // Closing tag
            if self.options.indent && has_element_children {
                for _ in 0..depth {
                    write!(self.writer, "{}", self.options.indent_str)?;
                }
            }
            write!(self.writer, "</{}>", qname)?;
        }

        Ok(())
    }

    fn write_escaped_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            match ch {
                '&' => write!(self.writer, "&amp;")?,
                '<' => write!(self.writer, "&lt;")?,
                '>' => write!(self.writer, "&gt;")?,
                _ => write!(self.writer, "{}", ch)?,
            }
        }
        Ok(())
    }
}

/// Escapes special characters for use in attribute values.
fn escape_attribute(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(ch),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_serialize_simple() {
        let doc = parse(r#"<root attr="value"><child>text</child></root>"#).unwrap();
        let root = doc.get_root_element().unwrap();
        let xml = node_to_xml_string(&doc, &root).unwrap();

        assert!(xml.contains("<root"));
        assert!(xml.contains("attr=\"value\""));
        assert!(xml.contains("<child>text</child>"));
        assert!(xml.contains("</root>"));
    }

    #[test]
    fn test_serialize_namespaced() {
        let doc = parse(
            r#"<gml:root xmlns:gml="http://www.opengis.net/gml">
            <gml:child>text</gml:child>
        </gml:root>"#,
        )
        .unwrap();
        let root = doc.get_root_element().unwrap();
        let xml = node_to_xml_string(&doc, &root).unwrap();

        assert!(xml.contains("xmlns:gml="));
        assert!(xml.contains("<gml:root"));
    }

    #[test]
    fn test_serialize_pretty() {
        let doc = parse(r#"<root><a/><b/></root>"#).unwrap();
        let root = doc.get_root_element().unwrap();
        let xml =
            node_to_xml_string_with_options(&doc, &root, &SerializeOptions::pretty()).unwrap();

        assert!(xml.contains('\n'));
    }

    #[test]
    fn test_escape_special_chars() {
        let doc = parse(r#"<root attr="&amp;test">&lt;text&gt;</root>"#).unwrap();
        let root = doc.get_root_element().unwrap();
        let xml = node_to_xml_string(&doc, &root).unwrap();

        assert!(xml.contains("&amp;") || xml.contains("&test"));
        assert!(xml.contains("&lt;") || xml.contains("<text>"));
    }

    #[test]
    fn test_printer_document_includes_declaration_by_default() {
        let doc = parse(r#"<root><child>hi</child></root>"#).unwrap();
        let xml = Printer::from(&doc).to_string().unwrap();
        assert!(xml.contains("<?xml"));
        assert!(xml.contains("<child>hi</child>"));
    }

    #[test]
    fn test_printer_node_has_no_declaration_by_default() {
        let doc = parse(r#"<root><child>hi</child></root>"#).unwrap();
        let root = doc.get_root_element().unwrap();
        let xml = Printer::from(&root).to_string().unwrap();
        assert!(!xml.contains("<?xml"));
        assert!(xml.contains("<root>"));
    }

    #[test]
    fn test_printer_pretty_and_declaration_toggle() {
        let doc = parse(r#"<root><a/><b/></root>"#).unwrap();
        let xml = Printer::from(&doc)
            .declaration(false)
            .pretty()
            .to_string()
            .unwrap();
        assert!(!xml.contains("<?xml"));
        assert!(xml.contains('\n'));
    }

    #[test]
    fn test_printer_write_to_and_into_bytes_match() {
        let doc = parse(r#"<root><a/></root>"#).unwrap();
        let bytes = Printer::from(&doc).into_bytes().unwrap();
        let mut buf = Vec::new();
        Printer::from(&doc).write_to(&mut buf).unwrap();
        assert_eq!(bytes, buf);
    }

    #[test]
    fn test_printer_readonly_node() {
        let doc = parse(r#"<root><child>hi</child></root>"#).unwrap();
        let root = doc.get_root_element().unwrap();
        let ro = XmlRoNode::from_node(root);
        let xml = Printer::from(&ro).to_string().unwrap();
        assert!(xml.contains("<child>hi</child>"));
    }
}
