//! Serialization API for EditableNode.

use std::collections::HashMap;

use crate::node::XmlNode;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};

use super::super::error::{TransformError, TransformResult};
use super::EditableNode;

impl EditableNode {
    /// Returns the XML string representation of this node.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("item", None, None, vec![("id", "1")], vec![], vec![]);
    /// builder.text("Hello");
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let xml = node.to_xml().unwrap();
    /// assert!(xml.contains("<item"));
    /// assert!(xml.contains("Hello"));
    /// ```
    pub fn to_xml(&self) -> TransformResult<String> {
        self.to_xml_with_options(&SerializeOptions::default())
    }

    /// Returns the XML string with custom serialization options.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// # use fastxml::serialize::SerializeOptions;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("item", None, None, vec![], vec![], vec![]);
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let options = SerializeOptions { indent: true, ..Default::default() };
    /// let xml = node.to_xml_with_options(&options).unwrap();
    /// ```
    pub fn to_xml_with_options(&self, options: &SerializeOptions) -> TransformResult<String> {
        let root = self
            .doc
            .get_root_element()
            .map_err(|e| TransformError::Serialization(e.to_string()))?;
        node_to_xml_string_with_options(&self.doc, &root, options)
            .map_err(|e| TransformError::Serialization(e.to_string()))
    }

    /// Returns the XML string with namespace declarations automatically added.
    ///
    /// This method analyzes the XML output to find all used namespace prefixes
    /// and adds the corresponding `xmlns:prefix` declarations to the root element.
    /// Only prefixes that are registered via `with_root_namespaces()` on the
    /// [`Transformer`](crate::transform::Transformer) will be included.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::Transformer;
    /// let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point/></root>"#;
    ///
    /// let mut fragment_xml = String::new();
    /// Transformer::from(xml)
    ///     .with_root_namespaces()
    ///     .unwrap()
    ///     .on("//gml:point", |node| {
    ///         fragment_xml = node.to_xml_with_namespaces().unwrap();
    ///     })
    ///     .for_each()
    ///     .unwrap();
    ///
    /// // The fragment includes the namespace declaration
    /// assert!(fragment_xml.contains("xmlns:gml"));
    /// assert!(fragment_xml.contains("http://www.opengis.net/gml"));
    /// ```
    pub fn to_xml_with_namespaces(&self) -> TransformResult<String> {
        self.to_xml_with_namespaces_and_options(&SerializeOptions::default())
    }

    /// Returns the XML string with namespace declarations and custom options.
    pub fn to_xml_with_namespaces_and_options(
        &self,
        options: &SerializeOptions,
    ) -> TransformResult<String> {
        let xml = self.to_xml_with_options(options)?;

        if self.namespaces.is_empty() {
            return Ok(xml);
        }

        // Collect prefixes used in the XML
        let used_prefixes = self.collect_used_prefixes();

        if used_prefixes.is_empty() {
            return Ok(xml);
        }

        // Build namespace declarations for used prefixes
        let mut ns_decls = Vec::new();
        for prefix in &used_prefixes {
            if let Some(uri) = self.namespaces.get(prefix) {
                // Check if the declaration already exists in the XML
                let decl_pattern = format!("xmlns:{}=", prefix);
                if !xml.contains(&decl_pattern) {
                    ns_decls.push(format!("xmlns:{}=\"{}\"", prefix, uri));
                }
            }
        }

        if ns_decls.is_empty() {
            return Ok(xml);
        }

        // Insert namespace declarations into the root element
        self.insert_namespace_declarations(&xml, &ns_decls)
    }

    /// Collects all namespace prefixes used in the element and its descendants.
    fn collect_used_prefixes(&self) -> Vec<String> {
        let mut prefixes = Vec::new();
        self.collect_prefixes_recursive(&self.root_node(), &mut prefixes);
        prefixes
    }

    /// Recursively collects namespace prefixes from a node and its children.
    fn collect_prefixes_recursive(&self, node: &XmlNode, prefixes: &mut Vec<String>) {
        if node.is_element() {
            if let Some(prefix) = node.get_prefix() {
                if !prefix.is_empty() && !prefixes.contains(&prefix) {
                    prefixes.push(prefix);
                }
            }
            // Also collect namespace prefixes from attributes
            for (local_name, _value) in node.get_attributes() {
                if let Some((attr_prefix, _uri)) = node.get_attribute_ns_info(&local_name) {
                    if !attr_prefix.is_empty() && !prefixes.contains(&attr_prefix) {
                        prefixes.push(attr_prefix);
                    }
                }
            }
        }

        for child in node.get_child_nodes() {
            self.collect_prefixes_recursive(&child, prefixes);
        }
    }

    /// Inserts namespace declarations into the first element tag.
    fn insert_namespace_declarations(
        &self,
        xml: &str,
        ns_decls: &[String],
    ) -> TransformResult<String> {
        // Find the position after the element name (before attributes or >)
        if let Some(first_lt) = xml.find('<') {
            let after_lt = &xml[first_lt + 1..];

            // Skip if it's a comment, PI, or closing tag
            if after_lt.starts_with('!') || after_lt.starts_with('?') || after_lt.starts_with('/') {
                return Ok(xml.to_string());
            }

            // Find the end of the element name (space, /, or >)
            if let Some(end_pos) =
                after_lt.find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            {
                let insert_pos = first_lt + 1 + end_pos;
                let ns_decl_str = format!(" {}", ns_decls.join(" "));

                let mut result = String::with_capacity(xml.len() + ns_decl_str.len());
                result.push_str(&xml[..insert_pos]);
                result.push_str(&ns_decl_str);
                result.push_str(&xml[insert_pos..]);
                return Ok(result);
            }
        }

        Ok(xml.to_string())
    }

    /// Returns the registered namespaces.
    pub fn namespaces(&self) -> &HashMap<String, String> {
        &self.namespaces
    }
}

impl std::fmt::Display for EditableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.to_xml() {
            Ok(xml) => write!(f, "{}", xml),
            Err(e) => write!(f, "<!-- Error: {} -->", e),
        }
    }
}

impl TryFrom<&EditableNode> for String {
    type Error = TransformError;

    /// Converts the node to an XML string.
    ///
    /// This is equivalent to calling `node.to_xml()`.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("item", None, None, vec![], vec![], vec![]);
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let xml: String = (&node).try_into().unwrap();
    /// assert!(xml.contains("<item"));
    /// ```
    fn try_from(node: &EditableNode) -> Result<String, Self::Error> {
        node.to_xml()
    }
}

impl TryFrom<EditableNode> for String {
    type Error = TransformError;

    /// Converts the node to an XML string, consuming the node.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use fastxml::transform::EditableNodeBuilder;
    /// let mut builder = EditableNodeBuilder::new();
    /// builder.start_element("item", None, None, vec![], vec![], vec![]);
    /// builder.end_element();
    /// let node = builder.build().unwrap();
    ///
    /// let xml: String = node.try_into().unwrap();
    /// assert!(xml.contains("<item"));
    /// ```
    fn try_from(node: EditableNode) -> Result<String, Self::Error> {
        node.to_xml()
    }
}
