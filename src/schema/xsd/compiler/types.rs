//! Type compilation - simple and complex type compilation.

use crate::error::Result;
use crate::schema::types::{AttributeDef, ComplexType, ContentModel, SimpleType, TypeDef};

use super::super::types::*;
use super::XsdCompiler;

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

                // Process facets
                for facet in &r.facets {
                    match facet {
                        XsdFacet::Enumeration(v) => {
                            compiled.enumeration.push(v.clone());
                        }
                        XsdFacet::Pattern(p) => {
                            compiled.pattern = Some(p.clone());
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
                        _ => {
                            // Other facets not yet supported
                        }
                    }
                }
            }
            XsdSimpleTypeContent::List(list) => {
                // List types - base type is the item type
                if let Some(item_type) = &list.item_type {
                    compiled.base_type = Some(format!("list({})", self.resolve_qname(item_type)));
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

        // Compile content model
        compiled.content = self.compile_complex_content(&ct.content)?;

        // Compile attributes
        for attr in &ct.attributes {
            compiled.attributes.push(self.compile_attribute(attr)?);
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
    pub(crate) fn compile_attribute(&self, attr: &XsdAttribute) -> Result<AttributeDef> {
        // Handle attribute reference
        if let Some(ref_qname) = &attr.ref_ {
            let mut compiled = AttributeDef::new(ref_qname.local.clone());
            compiled.required = attr.use_ == AttributeUse::Required;
            return Ok(compiled);
        }

        let name = attr.name.clone().unwrap_or_default();
        let mut compiled = AttributeDef::new(&name);

        if let Some(type_ref) = &attr.type_ref {
            compiled.type_ref = Some(self.resolve_qname(type_ref));
        }

        compiled.required = attr.use_ == AttributeUse::Required;
        compiled.default = attr.default.clone();
        compiled.fixed = attr.fixed.clone();

        Ok(compiled)
    }
}
