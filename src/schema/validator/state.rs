//! Validation state management during streaming.

use std::sync::Arc;

use smallvec::SmallVec;

use crate::namespace::Namespace;
use crate::schema::types::{ContentModelType, FlattenedChildren};

/// Validation state during streaming.
#[derive(Debug, Default)]
pub(crate) struct ValidationState {
    /// Current element stack (qname, namespace, occurrence count)
    pub element_stack: Vec<ElementContext>,
    /// Current depth
    pub depth: usize,
    /// Namespace bindings at each depth - stores only the diff for each scope
    /// This avoids cloning the entire HashMap on each push
    pub namespace_stack: Vec<SmallVec<[(String, String); 4]>>,
}

/// Context for an element being validated.
#[derive(Debug, Clone)]
pub(crate) struct ElementContext {
    /// Element name (local name) - stored as Arc<str> to avoid allocation
    pub name: Arc<str>,
    /// Element namespace URI (for future use)
    #[allow(dead_code)]
    pub namespace: Option<Arc<str>>,
    /// Child element occurrence counts - SmallVec for inline storage (most elements have <8 children)
    pub child_counts: SmallVec<[(Arc<str>, u32); 8]>,
    /// Text content collected
    pub text_content: String,
    /// Whether this element has been validated against schema
    pub schema_validated: bool,
    /// Type reference for this element (if known from schema)
    pub type_ref: Option<String>,
    /// Pre-computed flattened children constraints from schema cache.
    /// Using Arc to avoid cloning for every element - this is a reference to
    /// the pre-computed cache in CompiledSchema.
    pub flattened_children: Option<Arc<FlattenedChildren>>,
    /// Current position in expected sequence for sequence order validation.
    pub sequence_index: usize,
    /// Whether the element's declaration is `nillable="true"`. When true and the
    /// element is empty, primitive lexical/value-space checks are skipped so an
    /// `xsi:nil` element (e.g. an empty `xs:int`) is not rejected as invalid.
    pub nillable: bool,
    /// Whether the instance element carries `xsi:nil="true"`.
    pub nilled: bool,
    /// Default value from the element declaration (applies when empty).
    pub default_value: Option<String>,
    /// Fixed value from the element declaration.
    pub fixed_value: Option<String>,
    /// Inline (anonymous) type from the element declaration, used when the
    /// declaration carries no named type reference.
    pub inline_type: Option<crate::schema::types::TypeDef>,
    /// Set when this element was admitted by a lax/skip wildcard; its
    /// subtree inherits that processing mode.
    pub wildcard_mode: Option<crate::schema::types::ProcessContents>,
}

impl ElementContext {
    /// Creates a new ElementContext with an Arc<str> name (zero-copy from parser).
    pub fn new(name: Arc<str>, namespace: Option<Arc<str>>) -> Self {
        Self {
            name,
            namespace,
            child_counts: SmallVec::new(),
            text_content: String::new(),
            schema_validated: false,
            type_ref: None,
            flattened_children: None,
            sequence_index: 0,
            nillable: false,
            nilled: false,
            default_value: None,
            fixed_value: None,
            inline_type: None,
            wildcard_mode: None,
        }
    }

    /// Creates a new ElementContext from string references (allocates new strings).
    /// Used for tests and backward compatibility.
    #[allow(dead_code)]
    pub fn from_str(name: &str, namespace: Option<&str>) -> Self {
        Self::new(Arc::from(name), namespace.map(Arc::from))
    }

    /// Increments child count using Arc<str> (zero-copy if already tracked).
    pub fn increment_child_arc(&mut self, child_name: &Arc<str>) -> u32 {
        // Linear search is fast for small N (typically <8 children)
        for (name, count) in &mut self.child_counts {
            if Arc::ptr_eq(name, child_name) || name.as_ref() == child_name.as_ref() {
                *count += 1;
                return *count;
            }
        }
        self.child_counts.push((Arc::clone(child_name), 1));
        1
    }

    /// Increments child count from string reference (may allocate if new).
    #[allow(dead_code)]
    pub fn increment_child(&mut self, child_name: &str) -> u32 {
        // Linear search is fast for small N (typically <8 children)
        for (name, count) in &mut self.child_counts {
            if name.as_ref() == child_name {
                *count += 1;
                return *count;
            }
        }
        self.child_counts.push((Arc::from(child_name), 1));
        1
    }

    pub fn get_child_count(&self, child_name: &str) -> u32 {
        let mut total = 0;

        // Check if child_name has a prefix
        let has_prefix = child_name.contains(':');

        for (name, count) in &self.child_counts {
            if name.as_ref() == child_name {
                // Exact match
                total += count;
            } else if !has_prefix {
                // child_name is local name only - also match prefixed variants
                // e.g., child_name="LinearRing" should match "gml:LinearRing"
                if let Some((_prefix, local)) = name.split_once(':') {
                    if local == child_name {
                        total += count;
                    }
                }
            }
        }

        total
    }

    /// Returns the content model type from the flattened children, or Empty if not set.
    #[allow(dead_code)]
    pub fn content_model_type(&self) -> ContentModelType {
        self.flattened_children
            .as_ref()
            .map(|f| f.content_model_type)
            .unwrap_or(ContentModelType::Empty)
    }

    /// Checks if a child element is expected by this element.
    pub fn expects_child(&self, child_name: &str) -> bool {
        self.flattened_children
            .as_ref()
            .map(|f| f.constraints.contains_key(child_name))
            .unwrap_or(false)
    }

    /// Gets the constraint for a child element, if any.
    pub fn get_child_constraint(&self, child_name: &str) -> Option<(u32, Option<u32>)> {
        self.flattened_children
            .as_ref()
            .and_then(|f| f.constraints.get(child_name).copied())
    }

    /// Iterates over expected child constraints.
    #[allow(dead_code)]
    pub fn expected_children(&self) -> impl Iterator<Item = (&String, &(u32, Option<u32>))> {
        self.flattened_children
            .iter()
            .flat_map(|f| f.constraints.iter())
    }
}

impl ValidationState {
    pub fn new() -> Self {
        Self {
            element_stack: Vec::with_capacity(64),
            depth: 0,
            namespace_stack: vec![SmallVec::new()],
        }
    }

    /// Pushes a new element using Arc<str> name (zero-copy from parser).
    pub fn push_element(&mut self, name: Arc<str>, namespace: Option<Arc<str>>) {
        // Increment child count in parent using Arc<str> for zero-copy
        if let Some(parent) = self.element_stack.last_mut() {
            parent.increment_child_arc(&name);
        }
        self.element_stack
            .push(ElementContext::new(name, namespace));
        self.depth += 1;
    }

    /// Pushes a new element from string references (allocates).
    /// Used for tests and backward compatibility.
    #[allow(dead_code)]
    pub fn push_element_str(&mut self, name: &str, namespace: Option<&str>) {
        self.push_element(Arc::from(name), namespace.map(Arc::from));
    }

    pub fn pop_element(&mut self) -> Option<ElementContext> {
        self.depth = self.depth.saturating_sub(1);
        self.element_stack.pop()
    }

    #[allow(dead_code)]
    pub fn current_element(&self) -> Option<&ElementContext> {
        self.element_stack.last()
    }

    pub fn current_element_mut(&mut self) -> Option<&mut ElementContext> {
        self.element_stack.last_mut()
    }

    /// Pushes namespace declarations for the current scope.
    /// Only stores the diff (new bindings) instead of cloning the entire map.
    pub fn push_namespaces(&mut self, decls: &[Namespace]) {
        let bindings: SmallVec<[(String, String); 4]> = decls
            .iter()
            .map(|ns| (ns.prefix().to_string(), ns.uri().to_string()))
            .collect();
        self.namespace_stack.push(bindings);
    }

    pub fn pop_namespaces(&mut self) {
        if self.namespace_stack.len() > 1 {
            self.namespace_stack.pop();
        }
    }

    /// Resolves a namespace prefix by searching from innermost to outermost scope.
    #[allow(dead_code)]
    pub fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        // Search from innermost to outermost scope
        for scope in self.namespace_stack.iter().rev() {
            for (p, uri) in scope {
                if p == prefix {
                    return Some(uri.as_str());
                }
            }
        }
        None
    }

    /// Returns XPath-like path to current element.
    pub fn element_path(&self) -> String {
        if self.element_stack.is_empty() {
            return "/".to_string();
        }
        let mut path = String::new();
        for ctx in &self.element_stack {
            path.push('/');
            path.push_str(&ctx.name);
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_context_new() {
        let ctx = ElementContext::from_str("test", Some("http://example.com"));
        assert_eq!(ctx.name.as_ref(), "test");
        assert_eq!(ctx.namespace.as_deref(), Some("http://example.com"));
        assert!(ctx.child_counts.is_empty());
        assert!(ctx.text_content.is_empty());
        assert!(!ctx.schema_validated);
        assert!(ctx.type_ref.is_none());
        assert!(ctx.flattened_children.is_none());
    }

    #[test]
    fn test_element_context_new_without_namespace() {
        let ctx = ElementContext::from_str("test", None);
        assert_eq!(ctx.name.as_ref(), "test");
        assert!(ctx.namespace.is_none());
    }

    #[test]
    fn test_element_context_increment_child() {
        let mut ctx = ElementContext::from_str("parent", None);
        assert_eq!(ctx.increment_child("child"), 1);
        assert_eq!(ctx.increment_child("child"), 2);
        assert_eq!(ctx.increment_child("other"), 1);
        assert_eq!(ctx.increment_child("child"), 3);
    }

    #[test]
    fn test_element_context_get_child_count() {
        let mut ctx = ElementContext::from_str("parent", None);
        assert_eq!(ctx.get_child_count("child"), 0);
        ctx.increment_child("child");
        assert_eq!(ctx.get_child_count("child"), 1);
        ctx.increment_child("child");
        assert_eq!(ctx.get_child_count("child"), 2);
        assert_eq!(ctx.get_child_count("other"), 0);
    }

    #[test]
    fn test_validation_state_new() {
        let state = ValidationState::new();
        assert!(state.element_stack.is_empty());
        assert_eq!(state.depth, 0);
        assert_eq!(state.namespace_stack.len(), 1);
    }

    #[test]
    fn test_validation_state_push_pop_element() {
        let mut state = ValidationState::new();
        state.push_element_str("root", None);
        assert_eq!(state.depth, 1);
        assert_eq!(state.element_stack.len(), 1);

        state.push_element_str("child", Some("http://ns"));
        assert_eq!(state.depth, 2);
        assert_eq!(state.element_stack.len(), 2);

        let popped = state.pop_element();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().name.as_ref(), "child");
        assert_eq!(state.depth, 1);

        let popped = state.pop_element();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().name.as_ref(), "root");
        assert_eq!(state.depth, 0);
    }

    #[test]
    fn test_validation_state_child_count_increment() {
        let mut state = ValidationState::new();
        state.push_element_str("parent", None);
        state.push_element_str("child1", None);
        state.pop_element();
        state.push_element_str("child1", None);
        state.pop_element();
        state.push_element_str("child2", None);
        state.pop_element();

        // Parent should have counts: child1=2, child2=1
        let parent = state.pop_element().unwrap();
        assert_eq!(parent.get_child_count("child1"), 2);
        assert_eq!(parent.get_child_count("child2"), 1);
    }

    #[test]
    fn test_validation_state_current_element() {
        let mut state = ValidationState::new();
        assert!(state.current_element().is_none());

        state.push_element_str("root", None);
        assert!(state.current_element().is_some());
        assert_eq!(state.current_element().unwrap().name.as_ref(), "root");
    }

    #[test]
    fn test_validation_state_current_element_mut() {
        let mut state = ValidationState::new();
        state.push_element_str("root", None);
        if let Some(ctx) = state.current_element_mut() {
            ctx.text_content.push_str("hello");
        }
        assert_eq!(state.current_element().unwrap().text_content, "hello");
    }

    #[test]
    fn test_validation_state_push_pop_namespaces() {
        let mut state = ValidationState::new();
        assert_eq!(state.namespace_stack.len(), 1);

        let decls = vec![Namespace::new(
            "ns".to_string(),
            "http://example.com".to_string(),
        )];
        state.push_namespaces(&decls);
        assert_eq!(state.namespace_stack.len(), 2);
        assert_eq!(state.resolve_prefix("ns"), Some("http://example.com"));

        state.pop_namespaces();
        assert_eq!(state.namespace_stack.len(), 1);
    }

    #[test]
    fn test_validation_state_resolve_prefix() {
        let mut state = ValidationState::new();
        assert!(state.resolve_prefix("ns").is_none());

        let decls = vec![Namespace::new(
            "ns".to_string(),
            "http://example.com".to_string(),
        )];
        state.push_namespaces(&decls);
        assert_eq!(state.resolve_prefix("ns"), Some("http://example.com"));
        assert!(state.resolve_prefix("other").is_none());
    }

    #[test]
    fn test_validation_state_element_path_empty() {
        let state = ValidationState::new();
        assert_eq!(state.element_path(), "/");
    }

    #[test]
    fn test_validation_state_element_path() {
        let mut state = ValidationState::new();
        state.push_element_str("root", None);
        assert_eq!(state.element_path(), "/root");

        state.push_element_str("child", None);
        assert_eq!(state.element_path(), "/root/child");

        state.push_element_str("grandchild", None);
        assert_eq!(state.element_path(), "/root/child/grandchild");
    }

    #[test]
    fn test_validation_state_pop_empty() {
        let mut state = ValidationState::new();
        assert!(state.pop_element().is_none());
        assert_eq!(state.depth, 0);
    }

    #[test]
    fn test_namespace_stack_pop_minimum() {
        let mut state = ValidationState::new();
        assert_eq!(state.namespace_stack.len(), 1);
        state.pop_namespaces();
        // Should not pop below 1
        assert_eq!(state.namespace_stack.len(), 1);
        state.pop_namespaces();
        assert_eq!(state.namespace_stack.len(), 1);
    }

    /// Test that get_child_count matches local name even when stored with prefix.
    /// This is important for substitution group validation where we have:
    /// - XML element: `gml:LinearRing` (stored with prefix)
    /// - Schema substitution member: `LinearRing` (local name only)
    #[test]
    fn test_get_child_count_matches_local_name_with_prefix() {
        let mut ctx = ElementContext::from_str("parent", None);

        // Store child with prefix (as it comes from XML parser)
        ctx.increment_child("gml:LinearRing");

        // Should find it with exact match
        assert_eq!(ctx.get_child_count("gml:LinearRing"), 1);

        // Should also find it with local name only (for substitution group lookup)
        assert_eq!(
            ctx.get_child_count("LinearRing"),
            1,
            "get_child_count should match local name 'LinearRing' for prefixed child 'gml:LinearRing'"
        );
    }

    #[test]
    fn test_get_child_count_matches_local_name_multiple_prefixes() {
        let mut ctx = ElementContext::from_str("parent", None);

        // Store children with different prefixes but same local name
        ctx.increment_child("gml:Ring");
        ctx.increment_child("ns:Ring");

        // Exact match should find individual counts
        assert_eq!(ctx.get_child_count("gml:Ring"), 1);
        assert_eq!(ctx.get_child_count("ns:Ring"), 1);

        // Local name should find the sum of all prefixed versions
        assert_eq!(
            ctx.get_child_count("Ring"),
            2,
            "get_child_count with local name should sum all prefixed variants"
        );
    }
}
