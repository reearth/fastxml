//! Validation state management during streaming.

use std::collections::HashMap;

use crate::namespace::Namespace;

/// Type alias for child element occurrence constraints (min_occurs, max_occurs).
pub(crate) type ChildConstraints = HashMap<String, (u32, Option<u32>)>;

/// Content model type for an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ContentModelType {
    /// Sequence - all elements in order, each with their own min/max occurs
    #[default]
    Sequence,
    /// Choice - exactly one of the elements must be present
    Choice,
    /// All - all elements must be present, but in any order
    All,
    /// Empty - no child elements allowed
    Empty,
}

/// Validation state during streaming.
#[derive(Debug, Default)]
pub(crate) struct ValidationState {
    /// Current element stack (qname, namespace, occurrence count)
    pub element_stack: Vec<ElementContext>,
    /// Current depth
    pub depth: usize,
    /// Namespace bindings at each depth
    pub namespace_stack: Vec<HashMap<String, String>>,
}

/// Context for an element being validated.
#[derive(Debug, Clone)]
pub(crate) struct ElementContext {
    /// Element name (local name)
    pub name: String,
    /// Element namespace URI (for future use)
    #[allow(dead_code)]
    pub namespace: Option<String>,
    /// Child element occurrence counts
    pub child_counts: HashMap<String, u32>,
    /// Text content collected
    pub text_content: String,
    /// Whether this element has been validated against schema
    pub schema_validated: bool,
    /// Type reference for this element (if known from schema)
    pub type_ref: Option<String>,
    /// Expected child elements with their occurrence constraints (name -> (min, max))
    pub expected_children: ChildConstraints,
    /// Content model type (Sequence, Choice, All, or Empty)
    pub content_model_type: ContentModelType,
}

impl ElementContext {
    pub fn new(name: &str, namespace: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            namespace: namespace.map(|s| s.to_string()),
            child_counts: HashMap::new(),
            text_content: String::new(),
            schema_validated: false,
            type_ref: None,
            expected_children: HashMap::new(),
            content_model_type: ContentModelType::default(),
        }
    }

    pub fn increment_child(&mut self, child_name: &str) -> u32 {
        let count = self.child_counts.entry(child_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    pub fn get_child_count(&self, child_name: &str) -> u32 {
        *self.child_counts.get(child_name).unwrap_or(&0)
    }
}

impl ValidationState {
    pub fn new() -> Self {
        Self {
            element_stack: Vec::with_capacity(64),
            depth: 0,
            namespace_stack: vec![HashMap::new()],
        }
    }

    pub fn push_element(&mut self, name: &str, namespace: Option<&str>) {
        // Increment child count in parent
        if let Some(parent) = self.element_stack.last_mut() {
            parent.increment_child(name);
        }
        self.element_stack
            .push(ElementContext::new(name, namespace));
        self.depth += 1;
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

    pub fn push_namespaces(&mut self, decls: &[Namespace]) {
        let mut current = self.namespace_stack.last().cloned().unwrap_or_default();
        for ns in decls {
            current.insert(ns.prefix().to_string(), ns.uri().to_string());
        }
        self.namespace_stack.push(current);
    }

    pub fn pop_namespaces(&mut self) {
        if self.namespace_stack.len() > 1 {
            self.namespace_stack.pop();
        }
    }

    #[allow(dead_code)]
    pub fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        self.namespace_stack
            .last()
            .and_then(|ns| ns.get(prefix).map(|s| s.as_str()))
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
        let ctx = ElementContext::new("test", Some("http://example.com"));
        assert_eq!(ctx.name, "test");
        assert_eq!(ctx.namespace, Some("http://example.com".to_string()));
        assert!(ctx.child_counts.is_empty());
        assert!(ctx.text_content.is_empty());
        assert!(!ctx.schema_validated);
        assert!(ctx.type_ref.is_none());
        assert!(ctx.expected_children.is_empty());
    }

    #[test]
    fn test_element_context_new_without_namespace() {
        let ctx = ElementContext::new("test", None);
        assert_eq!(ctx.name, "test");
        assert!(ctx.namespace.is_none());
    }

    #[test]
    fn test_element_context_increment_child() {
        let mut ctx = ElementContext::new("parent", None);
        assert_eq!(ctx.increment_child("child"), 1);
        assert_eq!(ctx.increment_child("child"), 2);
        assert_eq!(ctx.increment_child("other"), 1);
        assert_eq!(ctx.increment_child("child"), 3);
    }

    #[test]
    fn test_element_context_get_child_count() {
        let mut ctx = ElementContext::new("parent", None);
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
        state.push_element("root", None);
        assert_eq!(state.depth, 1);
        assert_eq!(state.element_stack.len(), 1);

        state.push_element("child", Some("http://ns"));
        assert_eq!(state.depth, 2);
        assert_eq!(state.element_stack.len(), 2);

        let popped = state.pop_element();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().name, "child");
        assert_eq!(state.depth, 1);

        let popped = state.pop_element();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().name, "root");
        assert_eq!(state.depth, 0);
    }

    #[test]
    fn test_validation_state_child_count_increment() {
        let mut state = ValidationState::new();
        state.push_element("parent", None);
        state.push_element("child1", None);
        state.pop_element();
        state.push_element("child1", None);
        state.pop_element();
        state.push_element("child2", None);
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

        state.push_element("root", None);
        assert!(state.current_element().is_some());
        assert_eq!(state.current_element().unwrap().name, "root");
    }

    #[test]
    fn test_validation_state_current_element_mut() {
        let mut state = ValidationState::new();
        state.push_element("root", None);
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
        state.push_element("root", None);
        assert_eq!(state.element_path(), "/root");

        state.push_element("child", None);
        assert_eq!(state.element_path(), "/root/child");

        state.push_element("grandchild", None);
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
}
