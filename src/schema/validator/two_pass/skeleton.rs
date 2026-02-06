//! Element and document skeleton types for two-pass validation.

use std::collections::HashMap;
use std::sync::Arc;

/// A lightweight skeleton of an element for batch validation.
#[derive(Debug, Clone)]
pub struct ElementSkeleton {
    /// Element name (local name)
    pub name: Arc<str>,
    /// Element prefix (if any)
    pub prefix: Option<Arc<str>>,
    /// Child element occurrence counts - HashMap for efficient batch lookup
    pub child_counts: HashMap<String, u32>,
    /// Indices of child skeleton nodes in the flat storage
    pub children_indices: Vec<usize>,
    /// Line number for error reporting (1-indexed)
    pub line: Option<usize>,
    /// Column number for error reporting (1-indexed, in UTF-8 characters)
    pub column: Option<usize>,
    /// Collected text content
    pub text_content: String,
}

impl ElementSkeleton {
    /// Creates a new element skeleton.
    pub fn new(
        name: Arc<str>,
        prefix: Option<Arc<str>>,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Self {
        Self {
            name,
            prefix,
            child_counts: HashMap::new(),
            children_indices: Vec::new(),
            line,
            column,
            text_content: String::new(),
        }
    }

    /// Increments the count for a child element.
    pub fn increment_child(&mut self, child_name: &str) -> u32 {
        let count = self.child_counts.entry(child_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Gets the count for a child element.
    pub fn get_child_count(&self, child_name: &str) -> u32 {
        self.child_counts.get(child_name).copied().unwrap_or(0)
    }
}

/// A document skeleton containing all element skeletons in a flat structure.
#[derive(Debug, Default)]
pub struct DocumentSkeleton {
    /// Flat storage of all element skeletons
    pub nodes: Vec<ElementSkeleton>,
    /// Index of the root element (if any)
    pub root_index: Option<usize>,
}

impl DocumentSkeleton {
    /// Creates a new empty document skeleton.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            root_index: None,
        }
    }

    /// Gets a reference to a node by index.
    pub fn get_node(&self, index: usize) -> Option<&ElementSkeleton> {
        self.nodes.get(index)
    }
}
