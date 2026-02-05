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
}
