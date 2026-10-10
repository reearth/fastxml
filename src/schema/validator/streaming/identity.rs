//! Streaming identity constraint (unique / key / keyref) tracking.
//!
//! XSD restricts selector and field expressions to a small XPath subset
//! (XSD 1.0 §3.11.6): unions of relative paths, each optionally starting with
//! `.//`, made of `.` / name-test steps (`QName`, `*`, `prefix:*`, with an
//! optional `child::` axis), where a field may end in an attribute step
//! (`@name`, `@*`, `attribute::name`). That subset can be matched against the
//! element stack without building a DOM. Matched tuples are fed into the
//! shared [`ConstraintValidator`](crate::schema::xsd::constraints::ConstraintValidator),
//! whose keyref resolution runs at `finish()`.
//!
//! Name tests match by local name: the schema's prefixes are not resolved
//! against the instance's (the DOM engine resolves them through its XPath
//! engine). An expression outside the subset is not checked; the validator
//! reports a warning for it instead of skipping it silently.

use std::collections::HashSet;

use crate::error::{ErrorLevel, ValidationErrorType};
use crate::schema::types::{CompiledConstraint, CompiledConstraintType, ComplexType, TypeDef};
use crate::schema::xsd::constraints::{ConstraintType, IdentityConstraint, KeyValue};
use crate::schema::xsd::primitive::PrimitiveKind;

use super::super::state::ElementContext;
use super::OnePassSchemaValidator;

/// One union alternative of a selector or field: element steps below the
/// context node.
#[derive(Debug, Clone)]
pub(crate) struct StepPath {
    /// Leading `.//` — the steps may start at any depth at or below the
    /// context node.
    pub descendant: bool,
    /// Element step local names (`*` matches any element). Empty means the
    /// context node itself (`.`).
    pub steps: Vec<String>,
}

impl StepPath {
    /// Whether the relative element path (local names from just below the
    /// context node down to the candidate node; empty for the context node
    /// itself) is selected by this path.
    pub fn matches(&self, rel_path: &[&str]) -> bool {
        if self.descendant {
            rel_path.len() >= self.steps.len()
                && steps_match(&self.steps, &rel_path[rel_path.len() - self.steps.len()..])
        } else {
            rel_path.len() == self.steps.len() && steps_match(&self.steps, rel_path)
        }
    }
}

fn steps_match(steps: &[String], rel_path: &[&str]) -> bool {
    steps.iter().zip(rel_path).all(|(s, p)| s == "*" || s == p)
}

/// One union alternative of a field: an element path plus an optional
/// trailing attribute step.
#[derive(Debug, Clone)]
pub(crate) struct FieldAlt {
    pub path: StepPath,
    /// Trailing attribute step: a local name, or `*` for any attribute.
    pub attr: Option<String>,
}

/// A parsed field expression (its union alternatives).
#[derive(Debug, Clone)]
pub(crate) struct FieldPath {
    pub alts: Vec<FieldAlt>,
}

/// Parses a name test (`QName`, `*`, `prefix:*`) into the local name to match
/// (`*` for any). Returns `None` for anything else (predicates, functions,
/// other axes).
fn name_test(step: &str) -> Option<String> {
    let step = step.trim();
    if step.is_empty() {
        return None;
    }
    let valid = step
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '*') || !c.is_ascii());
    if !valid {
        return None;
    }
    let local = match step.split_once(':') {
        Some((_, local)) if !local.contains(':') => local,
        Some(_) => return None,
        None => step,
    };
    let starts_ok = |part: &str| {
        part.chars()
            .next()
            .is_some_and(|c| !(c.is_ascii_digit() || c == '.' || c == '-'))
    };
    if !starts_ok(step) || !starts_ok(local) || (local.contains('*') && local != "*") {
        return None;
    }
    Some(local.to_string())
}

/// Parses one union alternative into its element path and, when
/// `allow_attr`, an optional trailing attribute step.
fn parse_alt(alt: &str, allow_attr: bool) -> Option<(StepPath, Option<String>)> {
    let alt = alt.trim();
    let (descendant, rest) = match alt.strip_prefix(".//") {
        Some(rest) => (true, rest),
        None => (false, alt),
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let mut steps = Vec::new();
    let mut attr = None;
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if let Some(a) = part
            .strip_prefix('@')
            .or_else(|| part.strip_prefix("attribute::"))
        {
            if !allow_attr || i != parts.len() - 1 {
                return None; // an attribute step can only end a field
            }
            attr = Some(name_test(a)?);
        } else if part == "." {
            // self step: selects the current node, adds no element step
        } else {
            let name = part.strip_prefix("child::").unwrap_or(part);
            steps.push(name_test(name)?);
        }
    }
    Some((StepPath { descendant, steps }, attr))
}

/// Parses a selector into its union alternatives. Returns `None` for
/// expressions outside the XSD selector subset.
pub(crate) fn parse_selector(xpath: &str) -> Option<Vec<StepPath>> {
    xpath
        .split('|')
        .map(|alt| match parse_alt(alt, false)? {
            (path, None) => Some(path),
            (_, Some(_)) => None,
        })
        .collect()
}

/// Parses a field. Returns `None` for expressions outside the XSD field
/// subset.
pub(crate) fn parse_field(xpath: &str) -> Option<FieldPath> {
    let alts = xpath
        .split('|')
        .map(|alt| parse_alt(alt, true).map(|(path, attr)| FieldAlt { path, attr }))
        .collect::<Option<Vec<_>>>()?;
    Some(FieldPath { alts })
}

/// Whether an attribute (matched by local name) satisfies a field's attribute
/// step. `pattern` is `"*"` for the attribute wildcard (`@*` / `attribute::*`),
/// otherwise a literal local name.
pub(crate) fn attr_matches(attr_name: &str, pattern: &str) -> bool {
    pattern == "*" || attr_name.rsplit(':').next().unwrap_or(attr_name) == pattern
}

/// Captured value of one field for one selected node.
#[derive(Debug, Clone)]
pub(crate) enum FieldState {
    /// No node matched (yet): a missing field, i.e. null.
    Unset,
    /// Exactly one node matched, with this (canonicalized) value. An empty
    /// string is a value.
    Set(String),
    /// More than one node matched.
    Multiple,
    /// Exactly one node matched, but it is an element whose type has no
    /// simple value (element-only or mixed content).
    NotSimple,
}

impl FieldState {
    /// Records one more matching node.
    fn record(&mut self, value: String) {
        *self = match self {
            FieldState::Unset => FieldState::Set(value),
            _ => FieldState::Multiple,
        };
    }

    /// Records one more matching node that has no simple value.
    fn record_not_simple(&mut self) {
        *self = match self {
            FieldState::Unset => FieldState::NotSimple,
            _ => FieldState::Multiple,
        };
    }
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
    pub selector: Vec<StepPath>,
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
    /// `depth`. Returns `None` when its selector or a field is outside the
    /// supported subset.
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

    fn kind_name(&self) -> &'static str {
        match self.constraint.constraint_type {
            CompiledConstraintType::Key => "key",
            CompiledConstraintType::Unique => "unique",
            CompiledConstraintType::KeyRef => "keyref",
        }
    }
}

/// Local name of a (possibly prefixed) element name.
fn local_of(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

impl OnePassSchemaValidator {
    /// The complex type governing the current (most recently started)
    /// element, when one is resolvable from its declared or inline type.
    /// Used to resolve attribute value-space kinds for identity constraints.
    fn current_element_complex_type(&self) -> Option<&ComplexType> {
        let ctx = self.state.current_element()?;
        let type_def = match ctx.type_ref.as_deref() {
            // Namespace-qualified type identity first, string fallback.
            Some(tr) => self.schema.type_by_ref(ctx.type_ns.as_ref(), tr),
            None => ctx.inline_type.as_ref(),
        };
        match type_def {
            Some(TypeDef::Complex(c)) => Some(c),
            _ => None,
        }
    }

    /// Identity-constraint bookkeeping at element start: opens the scopes
    /// declared on this element, matches the element against the selectors
    /// of the scopes it is in, and captures attribute fields.
    pub(crate) fn identity_element_start(
        &mut self,
        elem_constraints: &[CompiledConstraint],
        attributes: &[(&str, &str)],
    ) {
        // With no identity scopes open and no constraints declared on this
        // element, there is nothing to match and nothing to open.
        if self.identity_scopes.is_empty() && elem_constraints.is_empty() {
            return;
        }

        let depth = self.state.element_stack.len();

        // Open scopes for constraints declared on this element first, so a
        // `.` selector can select the scoping element itself.
        for constraint in elem_constraints {
            match ScopeState::new(constraint, depth) {
                Some(scope) => self.identity_scopes.push(scope),
                None => self.warn_unsupported_constraint(constraint),
            }
        }

        // Resolve the value-space kind of each present attribute once, so the
        // identity-constraint field values captured below can be canonicalized
        // (e.g. the xs:integer attributes "1" and "01" denote the same key).
        let attr_kinds: Vec<Option<PrimitiveKind>> = {
            let complex = self.current_element_complex_type();
            attributes
                .iter()
                .map(|&(name, _)| {
                    complex.and_then(|c| {
                        super::super::attributes::attribute_primitive_kind(
                            &self.schema,
                            c,
                            local_of(name),
                        )
                    })
                })
                .collect()
        };

        // Local names of the elements on the stack (depth 1..=depth).
        let local_names: Vec<&str> = self
            .state
            .element_stack
            .iter()
            .map(|ctx| local_of(&ctx.name))
            .collect();

        for scope in &mut self.identity_scopes {
            // Selector match, relative to the scoping element.
            if depth >= scope.depth {
                let rel = &local_names[scope.depth..depth];
                if scope.selector.iter().any(|p| p.matches(rel)) {
                    scope.selected.push(SelectedState {
                        depth,
                        fields: vec![FieldState::Unset; scope.fields.len()],
                    });
                }
            }

            // Attribute fields on this element, relative to each selected
            // node at or above it (including one selected just now).
            for selected in &mut scope.selected {
                let rel = &local_names[selected.depth..depth];
                for (i, field) in scope.fields.iter().enumerate() {
                    for alt in &field.alts {
                        let Some(ref pattern) = alt.attr else {
                            continue;
                        };
                        if !alt.path.matches(rel) {
                            continue;
                        }
                        for (ai, &(n, v)) in attributes.iter().enumerate() {
                            if attr_matches(n, pattern) {
                                let canon = crate::schema::xsd::value_compare::identity_key(
                                    attr_kinds[ai],
                                    v,
                                );
                                selected.fields[i].record(canon);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Warns (once per constraint) that a constraint's selector or field is
    /// outside the subset the streaming engine evaluates, so it is not
    /// checked.
    fn warn_unsupported_constraint(&mut self, constraint: &CompiledConstraint) {
        if !self.unsupported_constraints.insert(constraint.name.clone()) {
            return;
        }
        let error = self
            .make_error(
                ValidationErrorType::IdentityConstraint,
                format!(
                    "identity constraint '{}' is not checked: its selector or field \
                     is outside the XPath subset XML Schema allows for identity constraints",
                    constraint.name
                ),
            )
            .with_level(ErrorLevel::Warning);
        self.add_error(error);
    }

    /// Identity-constraint bookkeeping at element end. `ended_depth` is the
    /// stack depth the element had; `ctx` is its popped context.
    pub(crate) fn identity_element_end(&mut self, ended_depth: usize, ctx: &ElementContext) {
        // Canonicalize the ending element's text in its value space so a
        // field selecting it compares correctly (e.g. xs:integer "01" and
        // "1" denote the same key).
        let text_kind: Option<PrimitiveKind> = if let Some(tr) = ctx.type_ref.as_deref() {
            self.schema
                .type_by_ref(ctx.type_ns.as_ref(), tr)
                .and_then(|td| {
                    super::super::attributes::element_text_primitive_kind(&self.schema, td)
                })
        } else if let Some(ref it) = ctx.inline_type {
            super::super::attributes::element_text_primitive_kind(&self.schema, it)
        } else {
            None
        };
        let text =
            crate::schema::xsd::value_compare::identity_key(text_kind, ctx.text_content.trim());
        // An element-only or mixed element cannot supply a field value.
        let has_simple_value = match ctx.type_ref.as_deref() {
            Some(tr) => self.schema.type_by_ref(ctx.type_ns.as_ref(), tr),
            None => ctx.inline_type.as_ref(),
        }
        .is_none_or(super::super::attributes::type_has_simple_value);

        // Local names of the still-open ancestors (depth 1..ended_depth)
        // followed by the ending element.
        let mut local_names: Vec<&str> = self
            .state
            .element_stack
            .iter()
            .map(|c| local_of(&c.name))
            .collect();
        local_names.push(local_of(&ctx.name));

        let mut errors: Vec<String> = Vec::new();

        for scope in &mut self.identity_scopes {
            // Element fields selecting the ending element, relative to each
            // selected node at or above it (including the node itself).
            for selected in &mut scope.selected {
                if selected.depth > ended_depth {
                    continue;
                }
                let rel = &local_names[selected.depth..ended_depth];
                for (i, field) in scope.fields.iter().enumerate() {
                    for alt in &field.alts {
                        if alt.attr.is_none() && alt.path.matches(rel) {
                            if has_simple_value {
                                selected.fields[i].record(text.clone());
                            } else {
                                selected.fields[i].record_not_simple();
                            }
                        }
                    }
                }
            }

            // Finalize selected nodes that end here.
            let mut finished = Vec::new();
            scope.selected.retain(|selected| {
                if selected.depth == ended_depth {
                    finished.push(selected.fields.clone());
                    false
                } else {
                    true
                }
            });

            for fields in finished {
                if fields.iter().any(|f| matches!(f, FieldState::Multiple)) {
                    errors.push(format!(
                        "{} '{}': a field matches more than one node",
                        scope.kind_name(),
                        scope.constraint.name
                    ));
                    continue;
                }
                if fields.iter().any(|f| matches!(f, FieldState::NotSimple)) {
                    errors.push(format!(
                        "{} '{}': a field selects an element without a simple value",
                        scope.kind_name(),
                        scope.constraint.name
                    ));
                    continue;
                }
                // A missing field (no node matched) is null: the tuple does
                // not take part in the constraint, except that a key
                // requires every field.
                if let Some(idx) = fields.iter().position(|f| matches!(f, FieldState::Unset)) {
                    if scope.constraint.constraint_type == CompiledConstraintType::Key {
                        errors.push(format!(
                            "null value in key field {} of constraint '{}'",
                            idx, scope.constraint.name
                        ));
                    }
                    continue;
                }
                let values: Vec<String> = fields
                    .into_iter()
                    .map(|f| match f {
                        FieldState::Set(v) => v,
                        _ => unreachable!("incomplete tuples are handled above"),
                    })
                    .collect();
                let value = KeyValue::new(values);
                let ic = IdentityConstraint {
                    name: scope.constraint.name.clone(),
                    constraint_type: match scope.constraint.constraint_type {
                        CompiledConstraintType::Unique => ConstraintType::Unique,
                        CompiledConstraintType::Key => ConstraintType::Key,
                        CompiledConstraintType::KeyRef => ConstraintType::KeyRef,
                    },
                    selector: scope.constraint.selector_xpath.clone(),
                    fields: scope.constraint.field_xpaths.clone(),
                    refer: scope.constraint.refer.clone(),
                };
                if scope.is_keyref() {
                    self.constraint_validator
                        .add_complete_keyref_tuple(&ic, value);
                } else {
                    // Uniqueness is per scoping-element instance: check
                    // against this scope's own `seen` set; the shared
                    // name-keyed table only serves keyref resolution.
                    if !scope.seen.insert(value.clone()) {
                        errors.push(format!(
                            "duplicate value {:?} in constraint '{}'",
                            value.values, scope.constraint.name
                        ));
                    }
                    self.constraint_validator
                        .record_complete_key_tuple(&ic, value);
                }
            }
        }

        // Close scopes whose scoping element ends here.
        self.identity_scopes
            .retain(|scope| scope.depth != ended_depth);

        for message in errors {
            let error = self
                .make_error(ValidationErrorType::IdentityConstraint, message)
                .with_level(ErrorLevel::Error);
            self.add_error(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(path: &StepPath) -> Vec<&str> {
        path.steps.iter().map(String::as_str).collect()
    }

    #[test]
    fn parses_selectors() {
        let p = parse_selector(".//tn:key").unwrap();
        assert!(p[0].descendant);
        assert_eq!(steps(&p[0]), ["key"]);

        let p = parse_selector("a/b | c").unwrap();
        assert_eq!(p.len(), 2);
        assert!(!p[0].descendant);
        assert_eq!(steps(&p[0]), ["a", "b"]);
        assert_eq!(steps(&p[1]), ["c"]);

        let p = parse_selector("./a/./child::b").unwrap();
        assert_eq!(steps(&p[0]), ["a", "b"]);

        let p = parse_selector(".").unwrap();
        assert!(p[0].steps.is_empty());

        let p = parse_selector("p:*").unwrap();
        assert_eq!(steps(&p[0]), ["*"]);
    }

    #[test]
    fn rejects_expressions_outside_the_subset() {
        assert!(parse_selector("a[@x]").is_none());
        assert!(parse_selector("a//b").is_none());
        assert!(parse_selector("@a").is_none());
        assert!(parse_selector("ancestor::a").is_none());
        assert!(parse_field("a/@b/c").is_none());
        assert!(parse_field("string(a)").is_none());
    }

    #[test]
    fn parses_fields() {
        let f = parse_field(".").unwrap();
        assert!(f.alts[0].path.steps.is_empty() && f.alts[0].attr.is_none());

        let f = parse_field("@val").unwrap();
        assert_eq!(f.alts[0].attr.as_deref(), Some("val"));

        let f = parse_field("a/b/@val").unwrap();
        assert_eq!(steps(&f.alts[0].path), ["a", "b"]);
        assert_eq!(f.alts[0].attr.as_deref(), Some("val"));

        let f = parse_field(".//v").unwrap();
        assert!(f.alts[0].path.descendant);
        assert_eq!(steps(&f.alts[0].path), ["v"]);

        let f = parse_field("@a | attribute::b").unwrap();
        assert_eq!(f.alts.len(), 2);
        assert_eq!(f.alts[1].attr.as_deref(), Some("b"));
    }

    #[test]
    fn matches_paths() {
        let sel = parse_selector(".//uid").unwrap();
        assert!(sel[0].matches(&["uid"]));
        assert!(sel[0].matches(&["a", "uid"]));
        assert!(!sel[0].matches(&["uid", "a"]));
        assert!(!sel[0].matches(&[]));

        let sel = parse_selector("a/b").unwrap();
        assert!(sel[0].matches(&["a", "b"]));
        assert!(!sel[0].matches(&["b"]));

        let sel = parse_selector(".").unwrap();
        assert!(sel[0].matches(&[]));
        assert!(!sel[0].matches(&["a"]));
    }
}
