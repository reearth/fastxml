//! Circular-definition detection over the XSD AST: `xs:group`,
//! `xs:attributeGroup`, and type-derivation cycles
//! (src-model_group / src-attribute_group / st-props-correct /
//! ct-props-correct circularity rules).

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::schema::error::SchemaError;

use super::super::types::*;
use super::references::tns_key;

/// Resolves a reference QName to a `(namespace, local)` key using the owning
/// schema's bindings, mirroring [`Checker::check_ref`]'s fallbacks. Only
/// returns keys present in `known`.
fn resolve_def_key(
    schema: &XsdSchema,
    qname: &QName,
    known: &HashSet<(String, String)>,
) -> Option<(String, String)> {
    let local = qname.local.trim().to_string();
    let ns = match qname.prefix.as_deref().map(str::trim) {
        Some("xml") => return None,
        Some(p) => schema.namespace_bindings.get(p)?.clone(),
        None => schema
            .namespace_bindings
            .get("")
            .cloned()
            .unwrap_or_default(),
    };
    let key = (ns, local.clone());
    if known.contains(&key) {
        return Some(key);
    }
    if !key.0.is_empty() {
        let chameleon = (String::new(), local.clone());
        if known.contains(&chameleon) {
            return Some(chameleon);
        }
    }
    if qname.prefix.is_none()
        && let Some(own) = &schema.target_namespace
    {
        let own_key = (own.clone(), local);
        if known.contains(&own_key) {
            return Some(own_key);
        }
    }
    None
}

/// Collects the group references appearing in a particle tree.
fn group_refs_in_particle<'a>(particle: &'a XsdParticle, out: &mut Vec<&'a QName>) {
    match particle {
        XsdParticle::Sequence(seq) => {
            for item in &seq.particles {
                group_refs_in_item(item, out);
            }
        }
        XsdParticle::Choice(choice) => {
            for item in &choice.particles {
                group_refs_in_item(item, out);
            }
        }
        XsdParticle::GroupRef(group_ref) => out.push(&group_ref.name),
        XsdParticle::All(_) | XsdParticle::Any(_) => {}
    }
}

fn group_refs_in_item<'a>(item: &'a XsdParticleItem, out: &mut Vec<&'a QName>) {
    match item {
        XsdParticleItem::Sequence(seq) => {
            for nested in &seq.particles {
                group_refs_in_item(nested, out);
            }
        }
        XsdParticleItem::Choice(choice) => {
            for nested in &choice.particles {
                group_refs_in_item(nested, out);
            }
        }
        XsdParticleItem::GroupRef(group_ref) => out.push(&group_ref.name),
        XsdParticleItem::Element(_) | XsdParticleItem::Any(_) => {}
    }
}

/// Collects the derivation-base references of a type definition (restriction
/// / extension bases, list item types, union member types), including those
/// of nested anonymous simple types.
fn type_base_refs<'a>(type_def: &'a XsdTypeDef, out: &mut Vec<&'a QName>) {
    match type_def {
        XsdTypeDef::Complex(ct) => match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => out.push(&ext.base),
                XsdSimpleContentDerivation::Restriction(r) => out.push(&r.base),
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => out.push(&ext.base),
                XsdComplexContentDerivation::Restriction(r) => out.push(&r.base),
            },
            XsdComplexContent::Empty | XsdComplexContent::Particle(_) => {}
        },
        XsdTypeDef::Simple(st) => simple_base_refs(st, out),
    }
}

fn simple_base_refs<'a>(st: &'a XsdSimpleType, out: &mut Vec<&'a QName>) {
    match &st.content {
        XsdSimpleTypeContent::Restriction(r) => {
            if let Some(base) = &r.base {
                out.push(base);
            }
            if let Some(inline) = &r.inline_base {
                simple_base_refs(inline, out);
            }
        }
        XsdSimpleTypeContent::List(list) => {
            if let Some(item) = &list.item_type {
                out.push(item);
            }
            if let Some(inline) = &list.inline_type {
                simple_base_refs(inline, out);
            }
        }
        XsdSimpleTypeContent::Union(union) => {
            // Union membership is not part of the derivation-cycle graph:
            // the W3C suite treats mutually-referencing unions as valid
            // (msData/simpleType/stE110).
            for inline in &union.inline_types {
                simple_base_refs(inline, out);
            }
        }
    }
}

/// Detects circular `xs:group` / `xs:attributeGroup` references and circular
/// type derivations (src-model_group / src-attribute_group /
/// st-props-correct / ct-props-correct circularity rules).
pub(super) fn check_definition_cycles(schemas: &[XsdSchema]) -> Result<()> {
    // (namespace, name) -> (owning schema index, outgoing references)
    let mut groups: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    let mut attr_groups: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    let mut types: HashMap<(String, String), (usize, Vec<&QName>)> = HashMap::new();
    for (si, schema) in schemas.iter().enumerate() {
        let ns = tns_key(schema);
        for group in &schema.groups {
            if let Some(name) = &group.name {
                let mut refs = Vec::new();
                if let Some(particle) = &group.particle {
                    group_refs_in_particle(particle, &mut refs);
                }
                groups.insert((ns.clone(), name.clone()), (si, refs));
            }
        }
        for ag in &schema.attribute_groups {
            if let Some(name) = &ag.name {
                let mut refs: Vec<&QName> = ag.attribute_groups.iter().collect();
                if let Some(r) = &ag.ref_ {
                    refs.push(r);
                }
                attr_groups.insert((ns.clone(), name.clone()), (si, refs));
            }
        }
        for type_def in &schema.types {
            if let Some(name) = type_def.name() {
                let mut refs = Vec::new();
                type_base_refs(type_def, &mut refs);
                types.insert((ns.clone(), name.to_string()), (si, refs));
            }
        }
    }
    detect_cycles("group", schemas, &groups)?;
    detect_cycles("attributeGroup", schemas, &attr_groups)?;
    detect_cycles("type derivation", schemas, &types)?;
    Ok(())
}

/// DFS cycle detection over a definition graph.
fn detect_cycles(
    kind: &str,
    schemas: &[XsdSchema],
    graph: &HashMap<(String, String), (usize, Vec<&QName>)>,
) -> Result<()> {
    let known: HashSet<(String, String)> = graph.keys().cloned().collect();
    // 1 = visiting, 2 = done
    let mut state: HashMap<&(String, String), u8> = HashMap::new();

    fn visit<'a>(
        kind: &str,
        key: &'a (String, String),
        schemas: &[XsdSchema],
        graph: &'a HashMap<(String, String), (usize, Vec<&QName>)>,
        known: &HashSet<(String, String)>,
        state: &mut HashMap<&'a (String, String), u8>,
    ) -> Result<()> {
        match state.get(key) {
            Some(1) => {
                return Err(SchemaError::InvalidSchema {
                    message: format!("circular {} reference involving '{}'", kind, key.1),
                }
                .into());
            }
            Some(_) => return Ok(()),
            None => {}
        }
        state.insert(key, 1);
        let (si, refs) = &graph[key];
        for qname in refs {
            if let Some(target) = resolve_def_key(&schemas[*si], qname, known)
                && let Some((target_key, _)) = graph.get_key_value(&target)
            {
                visit(kind, target_key, schemas, graph, known, state)?;
            }
        }
        state.insert(key, 2);
        Ok(())
    }

    for key in graph.keys() {
        visit(kind, key, schemas, graph, &known, &mut state)?;
    }
    Ok(())
}
