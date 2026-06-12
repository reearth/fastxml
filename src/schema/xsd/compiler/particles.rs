//! Particle compilation - sequences, choices, all, and elements.

use crate::error::Result;
use crate::schema::types::{
    CompiledConstraint, CompiledConstraintType, ContentModel, ElementDef, NsName, Particle,
    ProcessContents,
};

use super::super::types::*;
use super::XsdCompiler;

impl XsdCompiler {
    /// Compiles a particle to a content model.
    pub(crate) fn compile_particle(&mut self, particle: &XsdParticle) -> Result<ContentModel> {
        match particle {
            XsdParticle::Sequence(seq) => {
                let elements = self.compile_sequence(seq)?;
                Ok(ContentModel::Sequence(elements))
            }
            XsdParticle::Choice(choice) => {
                let elements = self.compile_choice(choice)?;
                Ok(ContentModel::Choice(elements))
            }
            XsdParticle::All(all) => {
                let elements = self.compile_all(all)?;
                Ok(ContentModel::All(elements))
            }
            XsdParticle::GroupRef(group_ref) => {
                // Expand the referenced named group inline. Reflect the group's
                // top-level compositor so sequence-order validation behaves like
                // the group had been declared directly.
                let elements = self.expand_group_ref_to_elements(group_ref)?;
                match self.resolve_group_particle(&group_ref.name) {
                    Some(XsdParticle::Choice(_)) => Ok(ContentModel::Choice(elements)),
                    Some(XsdParticle::All(_)) => Ok(ContentModel::All(elements)),
                    _ => Ok(ContentModel::Sequence(elements)),
                }
            }
            XsdParticle::Any(any) => Ok(ContentModel::Any {
                namespace: match &any.namespace {
                    NamespaceConstraint::Any => None,
                    NamespaceConstraint::Other => Some("##other".to_string()),
                    NamespaceConstraint::TargetNamespace => self.current_target_ns.clone(),
                    NamespaceConstraint::Local => Some("##local".to_string()),
                    NamespaceConstraint::List(uris) => Some(uris.join(" ")),
                },
                process_contents: match any.process_contents {
                    ProcessContentsMode::Strict => ProcessContents::Strict,
                    ProcessContentsMode::Lax => ProcessContents::Lax,
                    ProcessContentsMode::Skip => ProcessContents::Skip,
                },
            }),
        }
    }

    /// Compiles a particle to element definitions.
    pub(crate) fn compile_particle_to_elements(
        &mut self,
        particle: &XsdParticle,
    ) -> Result<Vec<ElementDef>> {
        match particle {
            XsdParticle::Sequence(seq) => self.compile_sequence(seq),
            XsdParticle::Choice(choice) => self.compile_choice(choice),
            XsdParticle::All(all) => self.compile_all(all),
            XsdParticle::GroupRef(group_ref) => self.expand_group_ref_to_elements(group_ref),
            XsdParticle::Any(_) => Ok(Vec::new()),
        }
    }

    /// Compiles a sequence.
    pub(crate) fn compile_sequence(&mut self, seq: &XsdSequence) -> Result<Vec<ElementDef>> {
        let mut elements = Vec::new();
        let seq_max = seq.max_occurs.to_option();
        let seq_min = match seq.min_occurs {
            Occurs::Count(n) => n,
            Occurs::Unbounded => 1,
        };

        for item in &seq.particles {
            match item {
                XsdParticleItem::Element(elem) => {
                    let mut compiled = self.compile_element(elem)?;
                    // Propagate sequence's maxOccurs to child element
                    compiled.max_occurs = Self::multiply_occurs(compiled.max_occurs, seq_max);
                    // A sequence repeated n times requires each child n
                    // times (and 0 makes children optional).
                    compiled.min_occurs = compiled.min_occurs.saturating_mul(seq_min);
                    elements.push(compiled);
                }
                XsdParticleItem::Sequence(nested) => {
                    let mut nested_elems = self.compile_sequence(nested)?;
                    // Propagate this sequence's occurs to nested results
                    for e in &mut nested_elems {
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, seq_max);
                        e.min_occurs = e.min_occurs.saturating_mul(seq_min);
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::Choice(nested) => {
                    let mut nested_elems = self.compile_choice(nested)?;
                    // Propagate this sequence's occurs to nested results
                    for e in &mut nested_elems {
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, seq_max);
                        e.min_occurs = e.min_occurs.saturating_mul(seq_min);
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::GroupRef(group_ref) => {
                    let mut group_elems = self.expand_group_ref_to_elements(group_ref)?;
                    // Propagate this sequence's occurs to the group's members.
                    for e in &mut group_elems {
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, seq_max);
                        e.min_occurs = e.min_occurs.saturating_mul(seq_min);
                    }
                    elements.extend(group_elems);
                }
                XsdParticleItem::Any(_) => {
                    // Any elements are handled elsewhere
                }
            }
        }

        Ok(elements)
    }

    /// Multiplies two maxOccurs values. If either is unbounded (None), result is unbounded.
    pub(crate) fn multiply_occurs(elem_max: Option<u32>, parent_max: Option<u32>) -> Option<u32> {
        match (elem_max, parent_max) {
            (None, _) | (_, None) => None, // unbounded
            (Some(a), Some(b)) => Some(a.saturating_mul(b)),
        }
    }

    /// Compiles a choice.
    pub(crate) fn compile_choice(&mut self, choice: &XsdChoice) -> Result<Vec<ElementDef>> {
        let mut elements = Vec::new();
        let choice_max = choice.max_occurs.to_option();

        for item in &choice.particles {
            match item {
                XsdParticleItem::Element(elem) => {
                    let mut compiled = self.compile_element(elem)?;
                    // Choice elements are implicitly optional
                    compiled.min_occurs = 0;
                    // Propagate choice's maxOccurs to child element
                    compiled.max_occurs = Self::multiply_occurs(compiled.max_occurs, choice_max);
                    elements.push(compiled);
                }
                XsdParticleItem::Sequence(nested) => {
                    let mut nested_elems = self.compile_sequence(nested)?;
                    for e in &mut nested_elems {
                        e.min_occurs = 0;
                        // Propagate choice's maxOccurs to nested results
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, choice_max);
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::Choice(nested) => {
                    let mut nested_elems = self.compile_choice(nested)?;
                    for e in &mut nested_elems {
                        // Propagate choice's maxOccurs to nested results
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, choice_max);
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::GroupRef(group_ref) => {
                    let mut group_elems = self.expand_group_ref_to_elements(group_ref)?;
                    for e in &mut group_elems {
                        // Choice members are implicitly optional.
                        e.min_occurs = 0;
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, choice_max);
                    }
                    elements.extend(group_elems);
                }
                XsdParticleItem::Any(_) => {}
            }
        }

        Ok(elements)
    }

    /// Resolves a group-ref QName to the referenced group's particle.
    ///
    /// The reference is resolved by namespace URI: an explicit prefix is mapped
    /// through the accumulated namespace bindings, otherwise the current target
    /// namespace is used. Returns a clone of the group's particle, if found.
    fn resolve_group_particle(&self, name: &QName) -> Option<XsdParticle> {
        let ns_uri = match &name.prefix {
            Some(prefix) => self.namespace_bindings.get(prefix).cloned(),
            None => self.current_target_ns.clone(),
        }
        .unwrap_or_default();
        self.groups
            .get(&NsName::new(ns_uri, name.local.clone()))
            .cloned()
    }

    /// Expands a `<xs:group ref>` into its member element definitions,
    /// propagating the reference site's own occurrence bounds and guarding
    /// against cyclic group references.
    fn expand_group_ref_to_elements(&mut self, group_ref: &XsdGroupRef) -> Result<Vec<ElementDef>> {
        let ns_uri = match &group_ref.name.prefix {
            Some(prefix) => self.namespace_bindings.get(prefix).cloned(),
            None => self.current_target_ns.clone(),
        }
        .unwrap_or_default();
        let key = NsName::new(ns_uri, group_ref.name.local.clone());

        let Some(particle) = self.groups.get(&key).cloned() else {
            tracing::debug!("Unresolved group reference: {}", group_ref.name);
            return Ok(Vec::new());
        };

        // Break cycles: a group that (transitively) references itself.
        if !self.group_expansion.insert(key.clone()) {
            return Ok(Vec::new());
        }
        let result = self.compile_particle_to_elements(&particle);
        self.group_expansion.remove(&key);
        let mut elements = result?;

        // Apply the occurrence bounds declared at the reference site. The group
        // is repeated [min, max] times, so each member's own bounds multiply by
        // the ref-site bounds: a member with min=j repeated N times needs j*N,
        // one with max=k repeated M times allows k*M. (minOccurs is never
        // "unbounded" in XSD; unwrap_or(1) is purely defensive.)
        let group_min = group_ref.min_occurs.to_option().unwrap_or(1);
        let group_max = group_ref.max_occurs.to_option();
        for e in &mut elements {
            e.min_occurs = e.min_occurs.saturating_mul(group_min);
            e.max_occurs = Self::multiply_occurs(e.max_occurs, group_max);
        }

        Ok(elements)
    }

    /// Compiles an all group.
    pub(crate) fn compile_all(&mut self, all: &XsdAll) -> Result<Vec<ElementDef>> {
        let mut elements = Vec::new();

        for elem in &all.elements {
            elements.push(self.compile_element(elem)?);
        }

        Ok(elements)
    }

    /// Compiles an element definition.
    pub(crate) fn compile_element(&mut self, elem: &XsdElement) -> Result<ElementDef> {
        // Handle element reference
        if let Some(ref_qname) = &elem.ref_ {
            let mut compiled = ElementDef::new(ref_qname.local.clone());
            compiled.min_occurs = elem.min_occurs.to_option().unwrap_or(1);
            compiled.max_occurs = elem.max_occurs.to_option();
            return Ok(compiled);
        }

        let mut compiled = ElementDef::new(&elem.name);

        // Set type reference
        if let Some(type_ref) = &elem.type_ref {
            compiled.type_ref = Some(self.resolve_qname(type_ref));
        }

        // Compile inline type
        if let Some(inline_type) = &elem.inline_type {
            compiled.inline_type = Some(self.compile_type(inline_type)?);
        }

        // Set occurrence bounds
        compiled.min_occurs = elem.min_occurs.to_option().unwrap_or(1);
        compiled.max_occurs = elem.max_occurs.to_option();

        // Set other properties
        compiled.is_abstract = elem.is_abstract;
        compiled.nillable = elem.nillable;
        compiled.default = elem.default.clone();
        compiled.fixed = elem.fixed.clone();

        if let Some(sg) = &elem.substitution_group {
            compiled.substitution_group = Some(self.resolve_qname(sg));
        }

        // Compile identity constraints (unique / key / keyref)
        for ic in &elem.identity_constraints {
            compiled.constraints.push(CompiledConstraint {
                name: ic.name.clone(),
                constraint_type: match ic.constraint_type {
                    XsdConstraintType::Unique => CompiledConstraintType::Unique,
                    XsdConstraintType::Key => CompiledConstraintType::Key,
                    XsdConstraintType::KeyRef => CompiledConstraintType::KeyRef,
                },
                selector_xpath: ic.selector.clone(),
                field_xpaths: ic.fields.clone(),
                refer: ic.refer.as_ref().map(|q| q.local.clone()),
            });
        }

        Ok(compiled)
    }

    /// Compiles a particle into the nested [`Particle`] tree used by the
    /// content-model automaton. Unlike [`Self::compile_particle`], the
    /// compositor structure and group occurrence bounds are preserved.
    ///
    /// Returns `None` when the tree cannot be faithfully represented (an
    /// unresolvable group reference) — the caller then skips building an
    /// automaton instead of building a wrong one.
    pub(crate) fn compile_particle_tree(
        &mut self,
        particle: &XsdParticle,
    ) -> Result<Option<Particle>> {
        let (min, max, items): (u32, Option<u32>, &[XsdParticleItem]) = match particle {
            XsdParticle::Sequence(seq) => (
                seq.min_occurs.to_option().unwrap_or(1),
                seq.max_occurs.to_option(),
                &seq.particles,
            ),
            XsdParticle::Choice(choice) => (
                choice.min_occurs.to_option().unwrap_or(1),
                choice.max_occurs.to_option(),
                &choice.particles,
            ),
            XsdParticle::All(all) => {
                let mut elements = Vec::new();
                for elem in &all.elements {
                    elements.push(self.compile_element(elem)?);
                }
                return Ok(Some(Particle::All {
                    min: all.min_occurs.to_option().unwrap_or(1),
                    elements,
                }));
            }
            XsdParticle::GroupRef(group_ref) => {
                return self.compile_group_ref_tree(group_ref);
            }
            XsdParticle::Any(any) => {
                let mut wc = self.compile_wildcard(any);
                wc.min_occurs = any.min_occurs.to_option().unwrap_or(1);
                wc.max_occurs = any.max_occurs.to_option();
                return Ok(Some(Particle::Wildcard(wc)));
            }
        };

        let mut children = Vec::with_capacity(items.len());
        for item in items {
            let child = match item {
                XsdParticleItem::Element(elem) => {
                    Some(Particle::Element(self.compile_element(elem)?))
                }
                XsdParticleItem::Sequence(nested) => {
                    self.compile_particle_tree(&XsdParticle::Sequence(nested.clone()))?
                }
                XsdParticleItem::Choice(nested) => {
                    self.compile_particle_tree(&XsdParticle::Choice(nested.clone()))?
                }
                XsdParticleItem::GroupRef(group_ref) => {
                    match self.compile_group_ref_tree(group_ref)? {
                        Some(p) => Some(p),
                        // Unresolvable group: give up on the whole tree.
                        None => return Ok(None),
                    }
                }
                XsdParticleItem::Any(any) => {
                    let mut wc = self.compile_wildcard(any);
                    wc.min_occurs = any.min_occurs.to_option().unwrap_or(1);
                    wc.max_occurs = any.max_occurs.to_option();
                    Some(Particle::Wildcard(wc))
                }
            };
            match child {
                Some(c) => children.push(c),
                None => return Ok(None),
            }
        }

        Ok(Some(match particle {
            XsdParticle::Choice(_) => Particle::Choice {
                min,
                max,
                items: children,
            },
            _ => Particle::Sequence {
                min,
                max,
                items: children,
            },
        }))
    }

    /// Resolves a group reference into its particle tree, applying the
    /// reference site's occurrence bounds to the group compositor.
    fn compile_group_ref_tree(&mut self, group_ref: &XsdGroupRef) -> Result<Option<Particle>> {
        let ns_uri = match &group_ref.name.prefix {
            Some(prefix) => self.namespace_bindings.get(prefix).cloned(),
            None => self.current_target_ns.clone(),
        }
        .unwrap_or_default();
        let key = NsName::new(ns_uri, group_ref.name.local.clone());

        let Some(particle) = self.groups.get(&key).cloned() else {
            return Ok(None);
        };
        // Break cycles like expand_group_ref_to_elements does.
        if !self.group_expansion.insert(key.clone()) {
            return Ok(None);
        }
        let result = self.compile_particle_tree(&particle);
        self.group_expansion.remove(&key);
        let Some(inner) = result? else {
            return Ok(None);
        };

        // The ref site's occurrence bounds repeat the whole group.
        let min = group_ref.min_occurs.to_option().unwrap_or(1);
        let max = group_ref.max_occurs.to_option();
        if min == 1 && max == Some(1) {
            return Ok(Some(inner));
        }
        Ok(Some(Particle::Sequence {
            min,
            max,
            items: vec![inner],
        }))
    }
}
