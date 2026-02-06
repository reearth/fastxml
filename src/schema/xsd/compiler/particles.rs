//! Particle compilation - sequences, choices, all, and elements.

use crate::error::Result;
use crate::schema::types::{ContentModel, ElementDef, ProcessContents};

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
            XsdParticle::GroupRef(qname) => {
                // Group references would need resolution
                tracing::debug!("Group reference: {}", qname);
                Ok(ContentModel::Empty)
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
            XsdParticle::GroupRef(_) => Ok(Vec::new()),
            XsdParticle::Any(_) => Ok(Vec::new()),
        }
    }

    /// Compiles a sequence.
    pub(crate) fn compile_sequence(&mut self, seq: &XsdSequence) -> Result<Vec<ElementDef>> {
        let mut elements = Vec::new();
        let seq_max = seq.max_occurs.to_option();
        let seq_min_zero = seq.min_occurs == Occurs::Count(0);

        for item in &seq.particles {
            match item {
                XsdParticleItem::Element(elem) => {
                    let mut compiled = self.compile_element(elem)?;
                    // Propagate sequence's maxOccurs to child element
                    compiled.max_occurs = Self::multiply_occurs(compiled.max_occurs, seq_max);
                    // If sequence is optional (minOccurs=0), child is also optional
                    if seq_min_zero {
                        compiled.min_occurs = 0;
                    }
                    elements.push(compiled);
                }
                XsdParticleItem::Sequence(nested) => {
                    let mut nested_elems = self.compile_sequence(nested)?;
                    // Propagate this sequence's occurs to nested results
                    for e in &mut nested_elems {
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, seq_max);
                        if seq_min_zero {
                            e.min_occurs = 0;
                        }
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::Choice(nested) => {
                    let mut nested_elems = self.compile_choice(nested)?;
                    // Propagate this sequence's occurs to nested results
                    for e in &mut nested_elems {
                        e.max_occurs = Self::multiply_occurs(e.max_occurs, seq_max);
                        if seq_min_zero {
                            e.min_occurs = 0;
                        }
                    }
                    elements.extend(nested_elems);
                }
                XsdParticleItem::GroupRef(_) => {
                    // Group references would need resolution
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
                XsdParticleItem::GroupRef(_) => {}
                XsdParticleItem::Any(_) => {}
            }
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

        if let Some(sg) = &elem.substitution_group {
            compiled.substitution_group = Some(self.resolve_qname(sg));
        }

        Ok(compiled)
    }
}
