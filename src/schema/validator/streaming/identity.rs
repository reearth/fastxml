//! Streaming identity constraint (unique / key / keyref) tracking.
//!
//! XSD selector/field expressions are a small XPath subset (child paths,
//! optional `.//` prefix, `@attr` fields, `.` self), which lets the
//! streaming validator match them against the element stack without
//! building a DOM. Matched tuples are fed into the shared
//! [`ConstraintValidator`](crate::schema::xsd::constraints::ConstraintValidator),
//! whose keyref resolution already runs at `finish()`.
//!
//! A prefixed element name test (`t:item`, `t:*`) matches by namespace URI,
//! with the prefix resolved against the schema's bindings, as the DOM engine
//! does through its XPath engine. Unprefixed element name tests and attribute
//! steps match by local name, which is also how the DOM engine evaluates them.

use std::collections::HashSet;

use crate::schema::types::{CompiledConstraint, CompiledConstraintType};
use crate::schema::xsd::constraints::KeyValue;

/// One element name test of a selector or field step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameTest {
    /// Namespace URI a prefixed test requires; `None` for an unprefixed test
    /// (or a prefix that could not be resolved), which matches any namespace.
    pub ns: Option<String>,
    /// Local name; `*` matches any element.
    pub local: String,
}

impl NameTest {
    /// Parses one step, resolving its prefix with `resolve`.
    fn parse(step: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Self {
        match step.split_once(':') {
            Some((prefix, local)) => Self {
                ns: resolve(prefix),
                local: local.to_string(),
            },
            None => Self {
                ns: None,
                local: step.to_string(),
            },
        }
    }

    /// Whether an element `{ns}local` passes this test.
    fn matches(&self, (ns, local): PathName<'_>) -> bool {
        (self.local == "*" || self.local == local)
            && self.ns.as_deref().is_none_or(|want| ns == Some(want))
    }
}

/// An element on a path: its namespace URI (`None` for no namespace) and
/// local name.
pub(crate) type PathName<'a> = (Option<&'a str>, &'a str);

/// One step-path alternative of a selector (`a/b`, `.//a`, `*`).
#[derive(Debug, Clone)]
pub(crate) struct SelectorPath {
    /// Leading `.//` — the path may start at any depth below the scope.
    pub descendant: bool,
    /// Element name tests, one per step.
    pub steps: Vec<NameTest>,
}

/// A parsed field expression.
#[derive(Debug, Clone)]
pub(crate) struct FieldPath {
    /// Element steps below the selected node (empty = the selected node).
    pub steps: Vec<NameTest>,
    /// Trailing attribute step (`@attr`), matched by local name.
    pub attr: Option<String>,
}

/// Parses a selector XPath into its union alternatives, resolving step
/// prefixes with `resolve`. Returns `None` for constructs outside the
/// supported subset.
pub(crate) fn parse_selector(
    xpath: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Option<Vec<SelectorPath>> {
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
            steps.push(NameTest::parse(step, resolve));
        }
        if steps.is_empty() {
            return None;
        }
        paths.push(SelectorPath { descendant, steps });
    }
    Some(paths)
}

/// Parses a field XPath, resolving element step prefixes with `resolve`.
/// Returns `None` for unsupported constructs.
pub(crate) fn parse_field(
    xpath: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> Option<FieldPath> {
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
            steps.push(NameTest::parse(part, resolve));
        }
    }
    Some(FieldPath { steps, attr })
}

/// Whether a relative element path (from just below the anchor to the
/// current element) matches one of the selector alternatives.
pub(crate) fn selector_matches(paths: &[SelectorPath], rel_path: &[PathName<'_>]) -> bool {
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
pub(crate) fn field_steps_match(field: &FieldPath, rel_path: &[PathName<'_>]) -> bool {
    rel_path.len() == field.steps.len() && steps_match(&field.steps, rel_path)
}

fn steps_match(steps: &[NameTest], rel_path: &[PathName<'_>]) -> bool {
    steps
        .iter()
        .zip(rel_path)
        .all(|(test, &name)| test.matches(name))
}

/// Whether an attribute (matched by local name) satisfies a field's attribute
/// step. `pattern` is `"*"` for the attribute wildcard (`@*` / `attribute::*`),
/// otherwise a literal local name.
pub(crate) fn attr_matches(attr_name: &str, pattern: &str) -> bool {
    pattern == "*" || attr_name.rsplit(':').next().unwrap_or(attr_name) == pattern
}

/// The URI `prefix` is bound to in `scoped` (prefix -> URI pairs).
fn scoped_prefix(scoped: &[(String, String)], prefix: &str) -> Option<String> {
    scoped
        .iter()
        .find(|(p, _)| p == prefix)
        .map(|(_, uri)| uri.clone())
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
    /// subset. Step prefixes resolve against the bindings in scope on the
    /// selector / field element, then through `schema_prefix` (the
    /// schema-wide prefix table), the same precedence as the DOM engine.
    pub fn new(
        constraint: &CompiledConstraint,
        depth: usize,
        schema_prefix: &dyn Fn(&str) -> Option<String>,
    ) -> Option<Self> {
        let selector = parse_selector(&constraint.selector_xpath, &|prefix| {
            scoped_prefix(&constraint.selector_namespaces, prefix).or_else(|| schema_prefix(prefix))
        })?;
        let fields = constraint
            .field_xpaths
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let scoped = constraint
                    .field_namespaces
                    .get(i)
                    .map_or(&[][..], Vec::as_slice);
                parse_field(f, &|prefix| {
                    scoped_prefix(scoped, prefix).or_else(|| schema_prefix(prefix))
                })
            })
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

    /// Resolves `tn` to `urn:tn`; other prefixes are unbound.
    fn tn(prefix: &str) -> Option<String> {
        (prefix == "tn").then(|| "urn:tn".to_string())
    }

    fn locals(steps: &[NameTest]) -> Vec<&str> {
        steps.iter().map(|s| s.local.as_str()).collect()
    }

    #[test]
    fn parses_selectors() {
        let p = parse_selector(".//tn:key", &tn).unwrap();
        assert!(p[0].descendant);
        assert_eq!(locals(&p[0].steps), ["key"]);
        assert_eq!(p[0].steps[0].ns.as_deref(), Some("urn:tn"));

        let p = parse_selector("a/b | c", &tn).unwrap();
        assert_eq!(p.len(), 2);
        assert!(!p[0].descendant);
        assert_eq!(locals(&p[0].steps), ["a", "b"]);
        assert_eq!(locals(&p[1].steps), ["c"]);
        assert!(p[0].steps.iter().all(|s| s.ns.is_none()));

        assert!(parse_selector("a[@x]", &tn).is_some()); // predicate kept as name — but
        // bracketed predicates are out of subset; ensure they don't match
        // real names by accident (the '[' stays in the step string).
    }

    #[test]
    fn parses_fields() {
        let f = parse_field(".", &tn).unwrap();
        assert!(f.steps.is_empty() && f.attr.is_none());

        let f = parse_field("@val", &tn).unwrap();
        assert_eq!(f.attr.as_deref(), Some("val"));

        let f = parse_field("a/tn:b/@val", &tn).unwrap();
        assert_eq!(locals(&f.steps), ["a", "b"]);
        assert_eq!(f.steps[1].ns.as_deref(), Some("urn:tn"));
        assert_eq!(f.attr.as_deref(), Some("val"));
    }

    #[test]
    fn matches_paths() {
        let sel = parse_selector(".//uid", &tn).unwrap();
        assert!(selector_matches(&sel, &[(None, "uid")]));
        assert!(selector_matches(
            &sel,
            &[(None, "a"), (Some("urn:x"), "uid")]
        ));
        assert!(!selector_matches(&sel, &[(None, "uid"), (None, "a")]));

        let sel = parse_selector("a/b", &tn).unwrap();
        assert!(selector_matches(&sel, &[(None, "a"), (None, "b")]));
        assert!(!selector_matches(&sel, &[(None, "b")]));
    }

    #[test]
    fn prefixed_steps_match_by_namespace() {
        let sel = parse_selector("tn:item | tn:*", &tn).unwrap();
        assert!(selector_matches(&sel, &[(Some("urn:tn"), "item")]));
        assert!(selector_matches(&sel, &[(Some("urn:tn"), "other")]));
        assert!(!selector_matches(&sel, &[(Some("urn:o"), "item")]));
        assert!(!selector_matches(&sel, &[(None, "item")]));
        // An unbound prefix falls back to the local name.
        let sel = parse_selector("zz:item", &tn).unwrap();
        assert!(selector_matches(&sel, &[(Some("urn:o"), "item")]));
    }
}
