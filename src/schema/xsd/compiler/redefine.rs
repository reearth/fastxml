//! xs:redefine application.
//!
//! Before compilation, each redefined component replaces the original under
//! its own name, and the original survives under a synthetic name
//! (`{name}__redefined`) that only the redefinition's rewritten
//! self-references point to. This matches xs:redefine semantics: within the
//! redefinition, a reference to the component's own name means the
//! *original* definition; everywhere else it means the redefinition.

use crate::schema::xsd::types::{
    XsdComplexContent, XsdComplexContentDerivation, XsdParticle, XsdParticleItem, XsdRedefine,
    XsdSchema, XsdSimpleContentDerivation, XsdSimpleTypeContent, XsdTypeDef,
};

/// Applies all xs:redefine declarations across the schema set, draining them.
pub(crate) fn apply_redefines(schemas: &mut [XsdSchema]) {
    let mut work: Vec<(usize, XsdRedefine)> = Vec::new();
    for (idx, schema) in schemas.iter_mut().enumerate() {
        for redefine in schema.redefines.drain(..) {
            work.push((idx, redefine));
        }
    }

    for (idx, redefine) in work {
        for mut ct in redefine.complex_types {
            let Some(name) = ct.name.clone() else {
                continue;
            };
            let synthetic = synthetic_name(&name);
            rename_type(schemas, &name, &synthetic, /* complex */ true);
            rewrite_complex_self_refs(&mut ct, &name, &synthetic);
            schemas[idx].types.push(XsdTypeDef::Complex(ct));
        }

        for mut st in redefine.simple_types {
            let Some(name) = st.name.clone() else {
                continue;
            };
            let synthetic = synthetic_name(&name);
            rename_type(schemas, &name, &synthetic, /* complex */ false);
            if let XsdSimpleTypeContent::Restriction(r) = &mut st.content
                && let Some(base) = &mut r.base
                && base.local == name
            {
                base.local = synthetic.clone();
            }
            schemas[idx].types.push(XsdTypeDef::Simple(st));
        }

        for mut group in redefine.groups {
            let Some(name) = group.name.clone() else {
                continue;
            };
            let synthetic = synthetic_name(&name);
            for schema in schemas.iter_mut() {
                if let Some(original) = schema
                    .groups
                    .iter_mut()
                    .find(|g| g.name.as_deref() == Some(name.as_str()))
                {
                    original.name = Some(synthetic.clone());
                    break;
                }
            }
            if let Some(particle) = &mut group.particle {
                rewrite_group_refs(particle, &name, &synthetic);
            }
            schemas[idx].groups.push(group);
        }

        for mut ag in redefine.attribute_groups {
            let Some(name) = ag.name.clone() else {
                continue;
            };
            let synthetic = synthetic_name(&name);
            for schema in schemas.iter_mut() {
                if let Some(original) = schema
                    .attribute_groups
                    .iter_mut()
                    .find(|g| g.name.as_deref() == Some(name.as_str()))
                {
                    original.name = Some(synthetic.clone());
                    break;
                }
            }
            for nested in &mut ag.attribute_groups {
                if nested.local == name {
                    nested.local = synthetic.clone();
                }
            }
            schemas[idx].attribute_groups.push(ag);
        }
    }
}

fn synthetic_name(name: &str) -> String {
    format!("{name}__redefined")
}

/// Renames the first matching top-level type declaration across the set.
fn rename_type(schemas: &mut [XsdSchema], name: &str, synthetic: &str, complex: bool) {
    for schema in schemas.iter_mut() {
        for type_def in schema.types.iter_mut() {
            match type_def {
                XsdTypeDef::Complex(ct) if complex && ct.name.as_deref() == Some(name) => {
                    ct.name = Some(synthetic.to_string());
                    return;
                }
                XsdTypeDef::Simple(st) if !complex && st.name.as_deref() == Some(name) => {
                    st.name = Some(synthetic.to_string());
                    return;
                }
                _ => {}
            }
        }
    }
}

/// Rewrites a redefined complex type's references to its own name (its
/// derivation base) to the synthetic original name.
fn rewrite_complex_self_refs(
    ct: &mut crate::schema::xsd::types::XsdComplexType,
    name: &str,
    synthetic: &str,
) {
    match &mut ct.content {
        XsdComplexContent::ComplexContent(cc) => match &mut cc.derivation {
            XsdComplexContentDerivation::Extension(ext) => {
                if ext.base.local == name {
                    ext.base.local = synthetic.to_string();
                }
            }
            XsdComplexContentDerivation::Restriction(r) => {
                if r.base.local == name {
                    r.base.local = synthetic.to_string();
                }
            }
        },
        XsdComplexContent::SimpleContent(sc) => match &mut sc.derivation {
            XsdSimpleContentDerivation::Extension(ext) => {
                if ext.base.local == name {
                    ext.base.local = synthetic.to_string();
                }
            }
            XsdSimpleContentDerivation::Restriction(r) => {
                if r.base.local == name {
                    r.base.local = synthetic.to_string();
                }
            }
        },
        _ => {}
    }
}

/// Rewrites `<xs:group ref>` self-references inside a redefined group.
fn rewrite_group_refs(particle: &mut XsdParticle, name: &str, synthetic: &str) {
    match particle {
        XsdParticle::Sequence(seq) => {
            for item in &mut seq.particles {
                rewrite_group_refs_item(item, name, synthetic);
            }
        }
        XsdParticle::Choice(choice) => {
            for item in &mut choice.particles {
                rewrite_group_refs_item(item, name, synthetic);
            }
        }
        XsdParticle::GroupRef(group_ref) => {
            if group_ref.name.local == name {
                group_ref.name.local = synthetic.to_string();
            }
        }
        XsdParticle::All(_) | XsdParticle::Any(_) => {}
    }
}

fn rewrite_group_refs_item(item: &mut XsdParticleItem, name: &str, synthetic: &str) {
    match item {
        XsdParticleItem::Sequence(seq) => {
            for nested in &mut seq.particles {
                rewrite_group_refs_item(nested, name, synthetic);
            }
        }
        XsdParticleItem::Choice(choice) => {
            for nested in &mut choice.particles {
                rewrite_group_refs_item(nested, name, synthetic);
            }
        }
        XsdParticleItem::GroupRef(group_ref) => {
            if group_ref.name.local == name {
                group_ref.name.local = synthetic.to_string();
            }
        }
        XsdParticleItem::Element(_) | XsdParticleItem::Any(_) => {}
    }
}
