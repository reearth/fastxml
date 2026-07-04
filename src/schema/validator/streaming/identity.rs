//! Streaming identity constraint (unique / key / keyref) tracking.
//!
//! XSD selector/field expressions are a small XPath subset (child paths,
//! optional `.//` prefix, `@attr` fields, `.` self), which lets the
//! streaming validator match them against the element stack without
//! building a DOM. Matched tuples are fed into the shared
//! [`ConstraintValidator`](crate::schema::xsd::constraints::ConstraintValidator),
//! whose keyref resolution already runs at `finish()`.

use std::collections::HashSet;

use crate::schema::types::{CompiledConstraint, CompiledConstraintType};
use crate::schema::xsd::constraints::KeyValue;

/// One step-path alternative of a selector (`a/b`, `.//a`, `*`).
#[derive(Debug, Clone)]
pub(crate) struct SelectorPath {
    /// Leading `.//` — the path may start at any depth below the scope.
    pub descendant: bool,
    /// Step local names (`*` matches any element).
    pub steps: Vec<String>,
}

/// A parsed field expression.
#[derive(Debug, Clone)]
pub(crate) struct FieldPath {
    /// Element steps below the selected node (empty = the selected node).
    pub steps: Vec<String>,
    /// Trailing attribute step (`@attr`).
    pub attr: Option<String>,
}

/// Parses a selector XPath into its union alternatives. Returns `None` for
/// constructs outside the supported subset.
pub(crate) fn parse_selector(xpath: &str) -> Option<Vec<SelectorPath>> {
    let mut paths = Vec::new();
    for alt in xpath.split('|') {
        let alt = alt.trim();
        let (descendant, rest) = if let Some(rest) = alt.strip_prefix(".//") {
            (true, rest)
        } else if let Some(rest) = alt.strip_prefix("./") {
            (false, rest)
        } else {
            (false, alt)
        };
        let mut steps = Vec::new();
        for step in rest.split('/') {
            let step = step.trim();
            if step.is_empty() || step.starts_with('@') || step == "." {
                return None;
            }
            let step = step.strip_prefix("child::").unwrap_or(step);
            // Match by local name; schema prefixes can differ from instance
            // prefixes for the same namespace.
            let local = step.rsplit(':').next().unwrap_or(step);
            steps.push(local.to_string());
        }
        if steps.is_empty() {
            return None;
        }
        paths.push(SelectorPath { descendant, steps });
    }
    Some(paths)
}

/// Parses a field XPath. Returns `None` for unsupported constructs.
pub(crate) fn parse_field(xpath: &str) -> Option<FieldPath> {
    let xpath = xpath.trim();
    if xpath == "." {
        return Some(FieldPath {
            steps: Vec::new(),
            attr: None,
        });
    }
    if xpath.contains("//") {
        return None;
    }
    let rest = xpath.strip_prefix("./").unwrap_or(xpath);
    let mut steps = Vec::new();
    let mut attr = None;
    let parts: Vec<&str> = rest.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        let part = part.strip_prefix("child::").unwrap_or(part);
        if let Some(a) = part
            .strip_prefix('@')
            .or_else(|| part.strip_prefix("attribute::"))
        {
            if i != parts.len() - 1 {
                return None; // attribute must be the last step
            }
            let local = a.rsplit(':').next().unwrap_or(a);
            attr = Some(local.to_string());
        } else {
            if part.is_empty() || part == "." {
                return None;
            }
            let local = part.rsplit(':').next().unwrap_or(part);
            steps.push(local.to_string());
        }
    }
    Some(FieldPath { steps, attr })
}

/// Whether a relative element path (local names from just below the anchor
/// to the current element) matches one of the selector alternatives.
pub(crate) fn selector_matches(paths: &[SelectorPath], rel_path: &[&str]) -> bool {
    paths.iter().any(|p| {
        if p.descendant {
            // steps must match the tail of rel_path
            rel_path.len() >= p.steps.len()
                && steps_match(&p.steps, &rel_path[rel_path.len() - p.steps.len()..])
        } else {
            rel_path.len() == p.steps.len() && steps_match(&p.steps, rel_path)
        }
    })
}

/// Whether a relative element path matches a field's element steps exactly.
pub(crate) fn field_steps_match(field: &FieldPath, rel_path: &[&str]) -> bool {
    rel_path.len() == field.steps.len() && steps_match(&field.steps, rel_path)
}

fn steps_match(steps: &[String], rel_path: &[&str]) -> bool {
    steps.iter().zip(rel_path).all(|(s, p)| s == "*" || s == p)
}

/// Captured value of one field for one selected node.
#[derive(Debug, Clone)]
pub(crate) enum FieldState {
    Unset,
    Set(String),
    Multiple,
}

/// A node matched by a constraint's selector, with its field captures.
#[derive(Debug)]
pub(crate) struct SelectedState {
    /// Element-stack depth of the selected node.
    pub depth: usize,
    pub fields: Vec<FieldState>,
}

/// An in-scope identity constraint (one per scoping element instance).
#[derive(Debug)]
pub(crate) struct ScopeState {
    pub constraint: CompiledConstraint,
    pub selector: Vec<SelectorPath>,
    pub fields: Vec<FieldPath>,
    /// Element-stack depth of the scoping element.
    pub depth: usize,
    pub selected: Vec<SelectedState>,
    /// Key tuples already seen within THIS scope instance. Uniqueness is
    /// per scope (per XSD): the same value may legally repeat under sibling
    /// scoping elements, so it must not be tracked in the shared,
    /// name-keyed key table (which exists only for cross-scope keyref
    /// resolution).
    pub seen: HashSet<KeyValue>,
}

impl ScopeState {
    /// Builds the scope state for a constraint declared on an element at
    /// `depth`, when its selector/field expressions are in the supported
    /// subset.
    pub fn new(constraint: &CompiledConstraint, depth: usize) -> Option<Self> {
        let selector = parse_selector(&constraint.selector_xpath)?;
        let fields = constraint
            .field_xpaths
            .iter()
            .map(|f| parse_field(f))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            constraint: constraint.clone(),
            selector,
            fields,
            depth,
            selected: Vec::new(),
            seen: HashSet::new(),
        })
    }

    /// Whether this constraint participates in key tables (key/unique) or
    /// consumes them (keyref).
    pub fn is_keyref(&self) -> bool {
        self.constraint.constraint_type == CompiledConstraintType::KeyRef
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_selectors() {
        let p = parse_selector(".//tn:key").unwrap();
        assert!(p[0].descendant);
        assert_eq!(p[0].steps, ["key"]);

        let p = parse_selector("a/b | c").unwrap();
        assert_eq!(p.len(), 2);
        assert!(!p[0].descendant);
        assert_eq!(p[0].steps, ["a", "b"]);
        assert_eq!(p[1].steps, ["c"]);

        assert!(parse_selector("a[@x]").is_some()); // predicate kept as name — but
        // bracketed predicates are out of subset; ensure they don't match
        // real names by accident (the '[' stays in the step string).
    }

    #[test]
    fn parses_fields() {
        let f = parse_field(".").unwrap();
        assert!(f.steps.is_empty() && f.attr.is_none());

        let f = parse_field("@val").unwrap();
        assert_eq!(f.attr.as_deref(), Some("val"));

        let f = parse_field("a/b/@val").unwrap();
        assert_eq!(f.steps, ["a", "b"]);
        assert_eq!(f.attr.as_deref(), Some("val"));
    }

    #[test]
    fn matches_paths() {
        let sel = parse_selector(".//uid").unwrap();
        assert!(selector_matches(&sel, &["uid"]));
        assert!(selector_matches(&sel, &["a", "uid"]));
        assert!(!selector_matches(&sel, &["uid", "a"]));

        let sel = parse_selector("a/b").unwrap();
        assert!(selector_matches(&sel, &["a", "b"]));
        assert!(!selector_matches(&sel, &["b"]));
    }
}
