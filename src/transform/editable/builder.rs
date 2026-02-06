//! Builder for creating EditableNode from parsed XML events.

use std::collections::HashMap;

use crate::document::DocumentBuilder;
use crate::namespace::Namespace;

use super::super::error::{TransformError, TransformResult};
use super::EditableNode;

/// Builder for creating EditableNode from parsed XML events.
pub struct EditableNodeBuilder {
    builder: DocumentBuilder,
    depth: usize,
    namespaces: HashMap<String, String>,
}

impl EditableNodeBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            builder: DocumentBuilder::new(),
            depth: 0,
            namespaces: HashMap::new(),
        }
    }

    /// Sets the namespace mappings for the built EditableNode.
    ///
    /// These namespaces will be available for use with `to_xml_with_namespaces()`.
    pub fn with_namespaces(mut self, namespaces: HashMap<String, String>) -> Self {
        self.namespaces = namespaces;
        self
    }

    /// Sets the namespace mappings (mutable reference version).
    pub fn set_namespaces(&mut self, namespaces: HashMap<String, String>) {
        self.namespaces = namespaces;
    }

    /// Adds a start element event.
    pub fn start_element(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        namespace_uri: Option<&str>,
        attributes: Vec<(&str, &str)>,
        attribute_ns_info: Vec<(&str, &str, &str)>,
        namespace_decls: Vec<Namespace>,
    ) {
        self.builder.start_element(
            name,
            prefix,
            namespace_uri,
            attributes,
            attribute_ns_info,
            namespace_decls,
            None,
            None,
        );
        self.depth += 1;
    }

    /// Adds an end element event.
    pub fn end_element(&mut self) {
        self.builder.end_element();
        self.depth = self.depth.saturating_sub(1);
    }

    /// Adds text content.
    pub fn text(&mut self, content: &str) {
        self.builder.text(content);
    }

    /// Adds CDATA content.
    pub fn cdata(&mut self, content: &str) {
        self.builder.cdata(content);
    }

    /// Adds a comment.
    pub fn comment(&mut self, content: &str) {
        self.builder.comment(content);
    }

    /// Returns true if the subtree is complete (depth returned to 0).
    pub fn is_complete(&self) -> bool {
        self.depth == 0
    }

    /// Returns the current depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Builds the EditableNode.
    pub fn build(self) -> TransformResult<EditableNode> {
        let doc = self.builder.build();
        let root_id = doc
            .root_element_id
            .ok_or_else(|| TransformError::XmlParse("no root element in subtree".to_string()))?;
        if self.namespaces.is_empty() {
            Ok(EditableNode::new(doc, root_id))
        } else {
            Ok(EditableNode::with_namespaces(doc, root_id, self.namespaces))
        }
    }
}

impl Default for EditableNodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
