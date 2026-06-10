//! Identity constraint (unique / key / keyref) evaluation for DOM validation.
//!
//! Constraints are collected during tree traversal (one task per scoping
//! element instance) and evaluated afterwards using the crate's XPath engine:
//! the selector picks the constrained nodes relative to the scoping element,
//! and each field yields one component of the tuple.

use std::collections::{HashMap, HashSet};

use crate::document::XmlDocument;
use crate::node::{NodeType, XmlNode};
use crate::schema::types::{CompiledConstraint, CompiledConstraintType};
use crate::xpath::{Query, XPathResult};

/// A scoping element instance paired with one of its declared constraints.
pub(crate) struct ConstraintTask {
    pub node: XmlNode,
    pub constraint: CompiledConstraint,
}

/// Evaluates all collected constraint tasks. Keys and uniques are processed
/// first so keyrefs can resolve against the key tables.
pub(crate) fn validate_identity_constraints(
    doc: &XmlDocument,
    tasks: &[ConstraintTask],
) -> Vec<String> {
    let mut errors = Vec::new();
    // Key/unique tables by constraint local name, for keyref resolution.
    let mut tables: HashMap<String, HashSet<Vec<String>>> = HashMap::new();

    for task in tasks {
        if task.constraint.constraint_type == CompiledConstraintType::KeyRef {
            continue;
        }
        let is_key = task.constraint.constraint_type == CompiledConstraintType::Key;
        let mut seen: HashSet<Vec<String>> = HashSet::new();

        for selected in select_nodes(doc, &task.node, &task.constraint.selector_xpath) {
            let outcomes: Vec<FieldOutcome> = task
                .constraint
                .field_xpaths
                .iter()
                .map(|f| field_value(doc, &selected, f))
                .collect();

            if outcomes.iter().any(|v| matches!(v, FieldOutcome::Multiple)) {
                errors.push(format!(
                    "{} '{}': a field matches more than one node",
                    if is_key { "key" } else { "unique" },
                    task.constraint.name
                ));
                continue;
            }
            if outcomes.iter().any(|v| matches!(v, FieldOutcome::Absent)) {
                if is_key {
                    errors.push(format!(
                        "key '{}': a field has no value for a selected node",
                        task.constraint.name
                    ));
                }
                continue; // incomplete tuples don't participate in uniqueness
            }
            let tuple: Vec<String> = outcomes
                .into_iter()
                .map(|v| match v {
                    FieldOutcome::Value(s) => s,
                    _ => unreachable!(),
                })
                .collect();

            if !seen.insert(tuple.clone()) {
                errors.push(format!(
                    "{} '{}': duplicate tuple [{}]",
                    if is_key { "key" } else { "unique" },
                    task.constraint.name,
                    tuple.join(", ")
                ));
            }
        }

        let local = task
            .constraint
            .name
            .rsplit(':')
            .next()
            .unwrap_or(&task.constraint.name);
        tables.entry(local.to_string()).or_default().extend(seen);
    }

    for task in tasks {
        if task.constraint.constraint_type != CompiledConstraintType::KeyRef {
            continue;
        }
        let Some(ref refer) = task.constraint.refer else {
            continue;
        };
        let refer_local = refer.rsplit(':').next().unwrap_or(refer);
        let Some(table) = tables.get(refer_local) else {
            // Referenced key was never established; per spec this should be
            // a schema error, but report unresolved references leniently.
            continue;
        };

        for selected in select_nodes(doc, &task.node, &task.constraint.selector_xpath) {
            let outcomes: Vec<FieldOutcome> = task
                .constraint
                .field_xpaths
                .iter()
                .map(|f| field_value(doc, &selected, f))
                .collect();
            if !outcomes.iter().all(|v| matches!(v, FieldOutcome::Value(_))) {
                continue; // incomplete keyref tuples are not checked
            }
            let tuple: Vec<String> = outcomes
                .into_iter()
                .map(|v| match v {
                    FieldOutcome::Value(s) => s,
                    _ => unreachable!(),
                })
                .collect();
            if !table.contains(&tuple) {
                errors.push(format!(
                    "keyref '{}': tuple [{}] does not match any '{}' key",
                    task.constraint.name,
                    tuple.join(", "),
                    refer_local
                ));
            }
        }
    }

    errors
}

/// Evaluates a selector XPath relative to `context`, returning element nodes.
fn select_nodes(doc: &XmlDocument, context: &XmlNode, xpath: &str) -> Vec<XmlNode> {
    let Ok(query) = Query::compile(xpath) else {
        return Vec::new();
    };
    match query.eval_from(doc, context) {
        Ok(XPathResult::Nodes(nodes)) => nodes,
        _ => Vec::new(),
    }
}

/// Outcome of evaluating one field XPath for one selected node.
enum FieldOutcome {
    /// Exactly one node matched (or a non-node value was produced)
    Value(String),
    /// No node matched
    Absent,
    /// More than one node matched - an identity constraint violation
    Multiple,
}

/// Evaluates a field XPath relative to a selected node. A field must select
/// at most one node (cvc-identity-constraint).
fn field_value(doc: &XmlDocument, context: &XmlNode, xpath: &str) -> FieldOutcome {
    let Ok(query) = Query::compile(xpath) else {
        return FieldOutcome::Absent;
    };
    match query.eval_from(doc, context) {
        Ok(XPathResult::Nodes(nodes)) => match nodes.as_slice() {
            [] => FieldOutcome::Absent,
            [node] => FieldOutcome::Value(string_value(node).trim().to_string()),
            _ => FieldOutcome::Multiple,
        },
        Ok(XPathResult::String(s)) => FieldOutcome::Value(s.trim().to_string()),
        Ok(XPathResult::Number(n)) => FieldOutcome::Value(n.to_string()),
        Ok(XPathResult::Boolean(b)) => FieldOutcome::Value(b.to_string()),
        Err(_) => FieldOutcome::Absent,
    }
}

/// XPath string-value of a node: its text content including descendants.
fn string_value(node: &XmlNode) -> String {
    match node.get_type() {
        NodeType::Element => {
            let mut out = String::new();
            collect_text(node, &mut out);
            out
        }
        _ => node.get_content().unwrap_or_default(),
    }
}

fn collect_text(node: &XmlNode, out: &mut String) {
    for child in node.get_child_nodes() {
        match child.get_type() {
            NodeType::Text | NodeType::CData => {
                if let Some(content) = child.get_content() {
                    out.push_str(&content);
                }
            }
            NodeType::Element => collect_text(&child, out),
            _ => {}
        }
    }
}
