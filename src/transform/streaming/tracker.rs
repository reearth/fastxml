//! Element path tracking and streaming XPath matching.
//!
//! [`PathTracker`] keeps the chain of open elements while a document is read
//! forward, together with the information a [`StreamableXPath`] needs to be
//! evaluated exactly on the current element:
//!
//! - the namespace declarations in scope (so element and attribute prefixes
//!   resolve against the document's own `xmlns` declarations),
//! - each open element's position among its preceding siblings, counted per
//!   kind of node test (`*`, local name, expanded name, …).
//!
//! The analyzer ([`super::super::xpath_analyze`]) only classifies an XPath as
//! streamable when every step, axis and predicate can be evaluated from this
//! information, so a streamable XPath selects the same elements as the DOM
//! evaluator.

use std::collections::HashMap;

use crate::xpath::parser::ComparisonOp;

use super::super::context::{AncestorInfo, TransformContext};
use super::super::xpath_analyze::{
    AttributePredicate, PositionPredicate, StreamableStep, StreamableXPath,
};

/// The namespace URI bound to the reserved `xml` prefix.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Tracks the current element path for XPath matching.
pub struct PathTracker {
    /// Stack of element info for current path
    path: Vec<ElementInfo>,
    /// Sibling positions of each element in `path` (parallel to `path`)
    positions: Vec<SiblingPositions>,
    /// Sibling counters per level; the last entry counts the children of the
    /// current element
    counters: Vec<LevelCounters>,
    /// Namespace declarations made on each element in `path` (parallel to `path`)
    ns_frames: Vec<Vec<(String, String)>>,
    /// Prefix bindings registered by the caller (used to resolve XPath prefixes)
    namespaces: HashMap<String, String>,
    /// Whether the per-node-test position counters are maintained
    track_positions: bool,
}

/// Information about an element in the path.
#[derive(Debug, Clone)]
pub struct ElementInfo {
    /// Local name
    pub name: String,
    /// Namespace prefix
    pub prefix: Option<String>,
    /// Namespace URI, resolved from the namespace declarations in scope in the
    /// document when the element is pushed onto a [`PathTracker`]
    pub namespace_uri: Option<String>,
    /// Attributes (qualified name -> value), including `xmlns` declarations
    pub attributes: HashMap<String, String>,
    /// Byte offset where this element starts (position of '<')
    pub start_offset: usize,
}

/// Counters of the children seen so far under one parent.
#[derive(Default)]
struct LevelCounters {
    any: usize,
    by_qname: HashMap<String, usize>,
    by_local: HashMap<String, usize>,
    by_expanded: HashMap<(Option<String>, String), usize>,
    by_uri: HashMap<Option<String>, usize>,
    by_prefix: HashMap<Option<String>, usize>,
}

/// 1-based positions of an element among its preceding siblings (inclusive),
/// for each kind of node test.
#[derive(Debug, Clone, Copy, Default)]
struct SiblingPositions {
    /// Among all element siblings (`*`)
    any: usize,
    /// Among siblings with the same qualified name (as written)
    qname: usize,
    /// Among siblings with the same local name (unprefixed name test)
    local: usize,
    /// Among siblings with the same namespace URI and local name (`p:name`)
    expanded: usize,
    /// Among siblings in the same namespace (`p:*`)
    uri: usize,
    /// Among siblings with the same literal prefix (`p:*` with an unbound prefix)
    prefix: usize,
}

fn bump<K: std::hash::Hash + Eq>(map: &mut HashMap<K, usize>, key: K) -> usize {
    let count = map.entry(key).or_insert(0);
    *count += 1;
    *count
}

fn qname_of(info: &ElementInfo) -> String {
    match &info.prefix {
        Some(p) => format!("{}:{}", p, info.name),
        None => info.name.clone(),
    }
}

impl PathTracker {
    /// Creates a new path tracker with no caller-registered namespaces.
    pub fn new() -> Self {
        Self::with_namespaces(HashMap::new())
    }

    /// Creates a path tracker that resolves XPath prefixes with `namespaces`
    /// (prefix -> URI) before falling back to the root element's declarations.
    pub fn with_namespaces(namespaces: HashMap<String, String>) -> Self {
        Self {
            path: Vec::new(),
            positions: Vec::new(),
            counters: vec![LevelCounters::default()],
            ns_frames: Vec::new(),
            namespaces,
            track_positions: true,
        }
    }

    /// Creates a tracker for matching `xpaths`, only maintaining the
    /// per-node-test position counters when one of them needs them.
    pub(crate) fn for_xpaths<'x>(
        namespaces: &HashMap<String, String>,
        xpaths: impl IntoIterator<Item = &'x StreamableXPath>,
    ) -> Self {
        let mut tracker = Self::with_namespaces(namespaces.clone());
        tracker.track_positions = xpaths.into_iter().any(|x| x.has_position_predicates());
        tracker
    }

    /// Pushes a new element onto the path stack.
    ///
    /// `xmlns` / `xmlns:p` entries in `info.attributes` open a namespace scope
    /// for this element and its descendants, and `info.namespace_uri` is
    /// resolved from that scope.
    pub fn push_element(&mut self, mut info: ElementInfo) {
        let mut frame = Vec::new();
        for (key, value) in &info.attributes {
            if key == "xmlns" {
                frame.push((String::new(), value.clone()));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                frame.push((prefix.to_string(), value.clone()));
            }
        }
        self.ns_frames.push(frame);

        let depth_index = self.ns_frames.len() - 1;
        let prefix_key = info.prefix.as_deref().unwrap_or("");
        if let Some(uri) = self.lookup_in_scope(prefix_key, depth_index) {
            info.namespace_uri = if uri.is_empty() {
                None
            } else {
                Some(uri.to_string())
            };
        }

        let qname = qname_of(&info);
        let level = self.counters.last_mut().expect("root level counter");
        level.any += 1;
        let mut pos = SiblingPositions {
            any: level.any,
            qname: bump(&mut level.by_qname, qname),
            ..Default::default()
        };
        if self.track_positions {
            pos.local = bump(&mut level.by_local, info.name.clone());
            pos.expanded = bump(
                &mut level.by_expanded,
                (info.namespace_uri.clone(), info.name.clone()),
            );
            pos.uri = bump(&mut level.by_uri, info.namespace_uri.clone());
            pos.prefix = bump(&mut level.by_prefix, info.prefix.clone());
        }

        self.path.push(info);
        self.positions.push(pos);
        // Add new level for children
        self.counters.push(LevelCounters::default());
    }

    /// Pops the current element from the path stack.
    pub fn pop_element(&mut self) {
        self.path.pop();
        self.positions.pop();
        self.ns_frames.pop();
        self.counters.pop();
    }

    /// Returns the current depth.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Returns the current element info (if any).
    pub fn current(&self) -> Option<&ElementInfo> {
        self.path.last()
    }

    /// Returns the current position of the latest element among siblings with the same name.
    pub fn current_position(&self) -> usize {
        self.positions.last().map_or(0, |p| p.qname)
    }

    /// Returns an XPath-like string representing the current path.
    ///
    /// The path includes position predicates for elements with siblings of the same name.
    /// Example: `/root/items[1]/item[3]`
    pub fn current_xpath(&self) -> String {
        if self.path.is_empty() {
            return String::new();
        }

        let parts: Vec<String> = self
            .path
            .iter()
            .zip(&self.positions)
            .map(|(info, pos)| format!("{}[{}]", qname_of(info), pos.qname))
            .collect();

        format!("/{}", parts.join("/"))
    }

    /// Creates a TransformContext from the current state.
    ///
    /// The context includes all ancestors (excluding the current element),
    /// the current position, and depth.
    pub fn to_context(&self) -> TransformContext {
        let parent_count = self.path.len().saturating_sub(1);
        let ancestors: Vec<AncestorInfo> = self.path[..parent_count]
            .iter()
            .zip(&self.positions)
            .enumerate()
            .map(|(i, (info, pos))| {
                AncestorInfo::new(
                    info.name.clone(),
                    info.prefix.clone(),
                    info.attributes.clone(),
                    pos.qname,
                    i + 1, // depth is 1-indexed
                )
            })
            .collect();

        TransformContext::new(ancestors, self.current_position(), self.depth())
    }

    /// Checks if the current element is selected by the streamable XPath.
    ///
    /// The XPath is evaluated the way the DOM evaluator does: absolute paths
    /// start at the document node, relative paths at the root element, and
    /// `//` (or `descendant::`) steps may skip any number of levels.
    pub fn matches(&self, xpath: &StreamableXPath) -> bool {
        let steps = &xpath.steps;
        let depth = self.path.len();
        if steps.is_empty() || depth == 0 {
            return false;
        }
        // Index in `path` of the context the first step is evaluated from:
        // the document node for absolute paths, the root element otherwise.
        let base = if xpath.absolute { 0 } else { 1 };
        if depth <= base {
            return false;
        }

        let last = steps.len() - 1;
        if !self.step_matches(&steps[last], depth - 1) {
            return false;
        }
        if !steps.iter().any(|s| s.descendant_or_self) && depth != base + steps.len() {
            return false;
        }
        if last == 0 {
            // Single step: already matched; only its depth is constrained
            return steps[0].descendant_or_self || depth - 1 == base;
        }

        // reachable[i]: steps[..=k] can select path[i]
        let mut reachable: Vec<bool> = (0..depth)
            .map(|i| {
                let positioned = if steps[0].descendant_or_self {
                    i >= base
                } else {
                    i == base
                };
                positioned && self.step_matches(&steps[0], i)
            })
            .collect();

        for step in &steps[1..] {
            let mut next = vec![false; depth];
            let mut any_before = false;
            for i in 0..depth {
                let from_context = if step.descendant_or_self {
                    any_before
                } else {
                    i >= 1 && reachable[i - 1]
                };
                next[i] = from_context && self.step_matches(step, i);
                any_before |= reachable[i];
            }
            reachable = next;
        }

        reachable[depth - 1]
    }

    /// Evaluates one step's node test and predicates against `path[index]`.
    fn step_matches(&self, step: &StreamableStep, index: usize) -> bool {
        let element = &self.path[index];

        // Node test
        let step_uri = step.prefix.as_deref().map(|p| self.resolve_xpath_prefix(p));
        match (&step.prefix, &step_uri) {
            (Some(_), Some(Some(uri))) => {
                if element.namespace_uri.as_deref() != Some(*uri) {
                    return false;
                }
            }
            (Some(prefix), _) => {
                // Prefix bound neither by the caller nor on the root element:
                // compare the prefix as written.
                if element.prefix.as_deref() != Some(prefix.as_str()) {
                    return false;
                }
            }
            (None, _) => {}
        }
        if let Some(ref name) = step.name {
            if element.name != *name {
                return false;
            }
        }

        // namespace-uri() = '...'
        if let Some(ref expected_uri) = step.namespace_uri {
            if element.namespace_uri.as_deref().unwrap_or("") != expected_uri {
                return false;
            }
        }

        // Position among the siblings selected by the same node test. The
        // analyzer only admits a position predicate as the first predicate of
        // a child step, so this is the XPath proximity position.
        if let Some(ref pos_pred) = step.position_predicate {
            let pos = &self.positions[index];
            let position = match (&step.prefix, &step.name, &step_uri) {
                (None, None, _) => pos.any,
                (None, Some(_), _) => pos.local,
                (Some(_), Some(_), Some(Some(_))) => pos.expanded,
                (Some(_), None, Some(Some(_))) => pos.uri,
                (Some(_), Some(_), _) => pos.qname,
                (Some(_), None, _) => pos.prefix,
            };
            if !matches_position_predicate(position, pos_pred) {
                return false;
            }
        }

        step.attribute_predicates
            .iter()
            .all(|pred| self.matches_attribute_predicate(index, pred))
    }

    fn matches_attribute_predicate(&self, index: usize, pred: &AttributePredicate) -> bool {
        let value = self.attribute_value(index, &pred.name);
        match pred.op {
            ComparisonOp::Equal => value == Some(pred.value.as_str()),
            ComparisonOp::NotEqual => {
                if pred.value.is_empty() {
                    // `[@attr]`: existence check
                    value.is_some()
                } else {
                    // A missing attribute is an empty node-set: `!=` is false
                    matches!(value, Some(v) if v != pred.value)
                }
            }
            _ => false,
        }
    }

    /// Looks up the attribute `name` (`local` or `prefix:local`) on `path[index]`.
    ///
    /// A prefixed name is matched by namespace URI: the XPath prefix is resolved
    /// like an element prefix, and each attribute's prefix in the document's
    /// namespace scope.
    fn attribute_value(&self, index: usize, name: &str) -> Option<&str> {
        let element = &self.path[index];
        let Some((prefix, local)) = name.split_once(':') else {
            if name == "xmlns" {
                return None;
            }
            return element.attributes.get(name).map(String::as_str);
        };

        let Some(uri) = self.resolve_xpath_prefix(prefix) else {
            return element.attributes.get(name).map(String::as_str);
        };

        element.attributes.iter().find_map(|(key, value)| {
            let (attr_prefix, attr_local) = key.split_once(':')?;
            if attr_local != local || attr_prefix == "xmlns" {
                return None;
            }
            (self.lookup_in_scope(attr_prefix, index) == Some(uri)).then_some(value.as_str())
        })
    }

    /// Resolves a prefix used in the XPath: caller-registered bindings first,
    /// then the reserved `xml` prefix, then the root element's declarations.
    fn resolve_xpath_prefix(&self, prefix: &str) -> Option<&str> {
        if let Some(uri) = self.namespaces.get(prefix) {
            return Some(uri.as_str());
        }
        if prefix == "xml" {
            return Some(XML_NAMESPACE);
        }
        self.ns_frames
            .first()?
            .iter()
            .find(|(p, _)| p == prefix)
            .map(|(_, uri)| uri.as_str())
    }

    /// Resolves a document prefix (`""` = default namespace) in the scope of
    /// `path[index]`.
    fn lookup_in_scope(&self, prefix: &str, index: usize) -> Option<&str> {
        if prefix == "xml" {
            return Some(XML_NAMESPACE);
        }
        self.ns_frames[..=index]
            .iter()
            .rev()
            .find_map(|frame| frame.iter().find(|(p, _)| p == prefix))
            .map(|(_, uri)| uri.as_str())
    }
}

fn matches_position_predicate(position: usize, pred: &PositionPredicate) -> bool {
    match pred {
        PositionPredicate::Exact(n) => position == *n,
        PositionPredicate::LessOrEqual(n) => position <= *n,
        PositionPredicate::LessThan(n) => position < *n,
        PositionPredicate::GreaterOrEqual(n) => position >= *n,
        PositionPredicate::GreaterThan(n) => position > *n,
    }
}

impl Default for PathTracker {
    fn default() -> Self {
        Self::new()
    }
}
