//! Types for editable node modifications.

use indexmap::IndexMap;

/// A modification to apply to a node.
#[derive(Debug, Clone)]
pub enum Modification {
    /// Set an attribute value
    SetAttribute {
        /// Attribute name
        name: String,
        /// Attribute value
        value: String,
    },
    /// Remove an attribute
    RemoveAttribute {
        /// Attribute name
        name: String,
    },
    /// Set the text content (replaces all children with a text node)
    SetTextContent(String),
    /// Append a new child element
    AppendChild(NewNode),
    /// Prepend a new child element
    PrependChild(NewNode),
    /// Replace text content while preserving structure
    ReplaceText {
        /// Old text to find
        old: String,
        /// New text to replace with
        new: String,
    },
}

/// A new node to be inserted.
#[derive(Debug, Clone)]
pub enum NewNode {
    /// A new element
    Element {
        /// Element name
        name: String,
        /// Namespace prefix
        prefix: Option<String>,
        /// Attributes (uses IndexMap to preserve insertion order)
        attributes: IndexMap<String, String>,
        /// Child nodes
        children: Vec<NewNode>,
    },
    /// A text node
    Text(String),
    /// A CDATA section
    CData(String),
    /// A comment
    Comment(String),
}
