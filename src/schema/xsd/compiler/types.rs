//! Type compilation - simple and complex type compilation.

use crate::error::Result;
use crate::schema::types::{
    AttributeDef, BlockSet, ComplexType, ContentModel, DerivationMethod, SimpleType, TypeDef,
    WhiteSpace,
};

use super::super::types::*;
use super::XsdCompiler;

/// Maps a parsed `block` attribute to the compiled [`BlockSet`].
fn compile_block_set(control: Option<&DerivationControl>) -> BlockSet {
    match control {
        None => BlockSet::default(),
        Some(DerivationControl::All) => BlockSet {
            extension: true,
            restriction: true,
        },
        Some(DerivationControl::List(types)) => BlockSet {
            extension: types.contains(&DerivationType::Extension),
            restriction: types.contains(&DerivationType::Restriction),
        },
    }
}

impl XsdCompiler {
    /// Compiles a type definition.
    pub(crate) fn compile_type(&mut self, type_def: &XsdTypeDef) -> Result<TypeDef> {
        match type_def {
            XsdTypeDef::Simple(st) => self.compile_simple_type(st),
            XsdTypeDef::Complex(ct) => self.compile_complex_type(ct),
        }
    }

    /// Compiles a simple type.
    pub(crate) fn compile_simple_type(&mut self, st: &XsdSimpleType) -> Result<TypeDef> {
        let name = st.name.clone().unwrap_or_default();
        let mut compiled = SimpleType::new(&name);

        match &st.content {
            XsdSimpleTypeContent::Restriction(r) => {
                if let Some(base) = &r.base {
                    compiled.base_type = Some(self.resolve_qname(base));
                }

                // Multiple pattern facets within one restriction step are
                // OR-ed per XSD; combine them into a single alternation.
                let mut patterns: Vec<String> = Vec::new();

                // Process facets
                for facet in &r.facets {
                    match facet {
                        XsdFacet::Enumeration(v) => {
                            compiled.enumeration.push(v.clone());
                        }
                        XsdFacet::Pattern(p) => {
                            patterns.push(p.clone());
                        }
                        XsdFacet::Length(n) => {
                            compiled.length = Some(*n);
                        }
                        XsdFacet::MinLength(n) => {
                            compiled.min_length = Some(*n);
                        }
                        XsdFacet::MaxLength(n) => {
                            compiled.max_length = Some(*n);
                        }
                        XsdFacet::MinInclusive(v) => {
                            compiled.min_inclusive = Some(v.clone());
                        }
                        XsdFacet::MaxInclusive(v) => {
                            compiled.max_inclusive = Some(v.clone());
                        }
                        XsdFacet::MinExclusive(v) => {
                            compiled.min_exclusive = Some(v.clone());
                        }
                        XsdFacet::MaxExclusive(v) => {
                            compiled.max_exclusive = Some(v.clone());
                        }
                        XsdFacet::TotalDigits(n) => {
                            compiled.total_digits = Some(*n);
                        }
                        XsdFacet::FractionDigits(n) => {
                            compiled.fraction_digits = Some(*n);
                        }
                        XsdFacet::WhiteSpace(v) => {
                            compiled.white_space = Some(match v {
                                WhiteSpaceValue::Preserve => WhiteSpace::Preserve,
                                WhiteSpaceValue::Replace => WhiteSpace::Replace,
                                WhiteSpaceValue::Collapse => WhiteSpace::Collapse,
                            });
                        }
                    }
                }

                if !patterns.is_empty() {
                    compiled.pattern = Some(if patterns.len() == 1 {
                        patterns.pop().unwrap()
                    } else {
                        patterns
                            .iter()
                            .map(|p| format!("(?:{})", p))
                            .collect::<Vec<_>>()
                            .join("|")
                    });
                }

                // Restriction of an inline simple type (e.g. restricting an
                // anonymous list): inherit its variety.
                if let Some(inline) = &r.inline_base {
                    if let Ok(TypeDef::Simple(inner)) = self.compile_simple_type(inline) {
                        compiled.item_type = inner.item_type;
                        if compiled.base_type.is_none() {
                            compiled.base_type = inner.base_type;
                        }
                    }
                }
            }
            XsdSimpleTypeContent::List(list) => {
                if let Some(item_type) = &list.item_type {
                    let resolved = self.resolve_qname(item_type);
                    compiled.item_type = Some(resolved.clone());
                    // Keep the legacy marker so PrimitiveKind::resolve does
                    // not accidentally walk into the item type: a list's
                    // value space is not its item's value space.
                    compiled.base_type = Some(format!("list({})", resolved));
                } else if let Some(inline) = &list.inline_type {
                    if let Ok(TypeDef::Simple(inner)) = self.compile_simple_type(inline) {
                        let item = inner.base_type.unwrap_or_default();
                        compiled.item_type = Some(item.clone());
                        compiled.base_type = Some(format!("list({})", item));
                    }
                }
            }
            XsdSimpleTypeContent::Union(union) => {
                // Union types - combine member types
                if !union.member_types.is_empty() {
                    let members: Vec<String> = union
                        .member_types
                        .iter()
                        .map(|q| self.resolve_qname(q))
                        .collect();
                    compiled.member_types = members.clone();
                    compiled.base_type = Some(format!("union({})", members.join(", ")));
                }
            }
        }

        Ok(TypeDef::Simple(compiled))
    }

    /// Compiles a complex type.
    pub(crate) fn compile_complex_type(&mut self, ct: &XsdComplexType) -> Result<TypeDef> {
        let name = ct.name.clone().unwrap_or_default();
        let mut compiled = ComplexType::new(&name);
        compiled.is_abstract = ct.is_abstract;
        compiled.mixed = ct.mixed;
        compiled.block = compile_block_set(ct.block.as_ref());

        // Record the derivation base and method (used for xsi:type checks)
        match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => {
                    compiled.base_type = Some(self.resolve_qname(&ext.base));
                    compiled.derivation = Some(DerivationMethod::Extension);
                }
                XsdSimpleContentDerivation::Restriction(r) => {
                    compiled.base_type = Some(self.resolve_qname(&r.base));
                    compiled.derivation = Some(DerivationMethod::Restriction);
                }
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => {
                    compiled.base_type = Some(self.resolve_qname(&ext.base));
                    compiled.derivation = Some(DerivationMethod::Extension);
                }
                XsdComplexContentDerivation::Restriction(r) => {
                    compiled.base_type = Some(self.resolve_qname(&r.base));
                    compiled.derivation = Some(DerivationMethod::Restriction);
                }
            },
            _ => {}
        }

        // Compile content model
        compiled.content = self.compile_complex_content(&ct.content)?;

        // Compile attributes
        for attr in &ct.attributes {
            compiled.attributes.push(self.compile_attribute(attr)?);
        }

        // Attributes declared inside simpleContent / complexContent
        // derivations belong to this type as well.
        let derivation_attrs: Option<&[XsdAttribute]> = match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => Some(&ext.attributes),
                XsdSimpleContentDerivation::Restriction(r) => Some(&r.attributes),
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => Some(&ext.attributes),
                XsdComplexContentDerivation::Restriction(r) => Some(&r.attributes),
            },
            _ => None,
        };
        if let Some(attrs) = derivation_attrs {
            for attr in attrs {
                compiled.attributes.push(self.compile_attribute(attr)?);
            }
        }

        // Handle attribute groups
        for ag_ref in &ct.attribute_groups {
            // Attribute groups would need resolution - for now just note them
            tracing::debug!("Attribute group reference: {}", ag_ref);
        }

        Ok(TypeDef::Complex(compiled))
    }

    /// Compiles complex type content.
    pub(crate) fn compile_complex_content(
        &mut self,
        content: &XsdComplexContent,
    ) -> Result<ContentModel> {
        match content {
            XsdComplexContent::Empty => Ok(ContentModel::Empty),

            XsdComplexContent::Particle(particle) => self.compile_particle(particle),

            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => Ok(ContentModel::SimpleContent {
                    base_type: self.resolve_qname(&ext.base),
                }),
                XsdSimpleContentDerivation::Restriction(r) => Ok(ContentModel::SimpleContent {
                    base_type: self.resolve_qname(&r.base),
                }),
            },

            XsdComplexContent::ComplexContent(cc) => {
                match &cc.derivation {
                    XsdComplexContentDerivation::Extension(ext) => {
                        let elements = ext
                            .particle
                            .as_ref()
                            .map(|p| self.compile_particle_to_elements(p))
                            .transpose()?
                            .unwrap_or_default();

                        Ok(ContentModel::ComplexExtension {
                            base_type: self.resolve_qname(&ext.base),
                            elements,
                        })
                    }
                    XsdComplexContentDerivation::Restriction(r) => {
                        // Restriction redefines content
                        if let Some(particle) = &r.particle {
                            self.compile_particle(particle)
                        } else {
                            Ok(ContentModel::Empty)
                        }
                    }
                }
            }
        }
    }

    /// Compiles an attribute definition.
    pub(crate) fn compile_attribute(&mut self, attr: &XsdAttribute) -> Result<AttributeDef> {
        // Handle attribute reference
        if let Some(ref_qname) = &attr.ref_ {
            let mut compiled = AttributeDef::new(ref_qname.local.clone());
            compiled.required = attr.use_ == AttributeUse::Required;
            compiled.is_ref = true;
            return Ok(compiled);
        }

        let name = attr.name.clone().unwrap_or_default();
        let mut compiled = AttributeDef::new(&name);

        if let Some(type_ref) = &attr.type_ref {
            compiled.type_ref = Some(self.resolve_qname(type_ref));
        } else if let Some(inline) = &attr.inline_type {
            if let TypeDef::Simple(simple) = self.compile_simple_type(inline)? {
                compiled.inline_type = Some(Box::new(simple));
            }
        }

        compiled.required = attr.use_ == AttributeUse::Required;
        compiled.default = attr.default.clone();
        compiled.fixed = attr.fixed.clone();

        Ok(compiled)
    }
}
