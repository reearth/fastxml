//! Type compilation - simple and complex type compilation.

use crate::error::Result;
use crate::schema::types::{
    AttributeDef, BlockSet, ComplexType, ContentModel, DerivationMethod, ProcessContents,
    SimpleType, TypeDef, WhiteSpace, WildcardConstraint, WildcardNamespace,
};

use super::super::types::*;
use super::XsdCompiler;

/// A wildcard found in a content model along with its effective occurrence
/// bounds (the wildcard's own occurs scaled by every enclosing compositor).
struct FoundWildcard<'a> {
    any: &'a XsdAny,
    min: u32,
    max: Option<u32>,
}

impl<'a> FoundWildcard<'a> {
    fn scaled(mut self, min: Occurs, max: Occurs, optional: bool) -> Self {
        let comp_min = match min {
            Occurs::Count(n) => n,
            Occurs::Unbounded => 1,
        };
        self.min = if optional {
            0
        } else {
            self.min.saturating_mul(comp_min)
        };
        let comp_max = match max {
            Occurs::Count(n) => Some(n),
            Occurs::Unbounded => None,
        };
        self.max = match (self.max, comp_max) {
            (Some(a), Some(b)) => Some(a.saturating_mul(b)),
            _ => None,
        };
        self
    }
}

/// Finds the first element wildcard (`xs:any`) in a complex type's content.
fn find_wildcard_in_content(content: &XsdComplexContent) -> Option<FoundWildcard<'_>> {
    match content {
        XsdComplexContent::Particle(p) => find_wildcard_in_particle(p),
        XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
            XsdComplexContentDerivation::Extension(ext) => {
                ext.particle.as_ref().and_then(find_wildcard_in_particle)
            }
            XsdComplexContentDerivation::Restriction(r) => {
                r.particle.as_ref().and_then(find_wildcard_in_particle)
            }
        },
        _ => None,
    }
}

fn found(any: &XsdAny) -> FoundWildcard<'_> {
    FoundWildcard {
        any,
        min: match any.min_occurs {
            Occurs::Count(n) => n,
            Occurs::Unbounded => 0,
        },
        max: match any.max_occurs {
            Occurs::Count(n) => Some(n),
            Occurs::Unbounded => None,
        },
    }
}

fn find_wildcard_in_particle(particle: &XsdParticle) -> Option<FoundWildcard<'_>> {
    match particle {
        XsdParticle::Any(any) => Some(found(any)),
        XsdParticle::Sequence(seq) => find_wildcard_in_items(&seq.particles)
            .map(|w| w.scaled(seq.min_occurs, seq.max_occurs, false)),
        XsdParticle::Choice(choice) => {
            find_wildcard_in_items(&choice.particles).map(|w| {
                // With sibling alternatives the wildcard branch may be
                // skipped entirely, so its minimum is not enforceable.
                w.scaled(
                    choice.min_occurs,
                    choice.max_occurs,
                    choice.particles.len() > 1,
                )
            })
        }
        XsdParticle::All(_) => None, // xs:all cannot contain wildcards in XSD 1.0
        XsdParticle::GroupRef(_) => None,
    }
}

fn find_wildcard_in_items(items: &[XsdParticleItem]) -> Option<FoundWildcard<'_>> {
    let optional = items.len() > 1; // siblings make exact bounds undecidable
    for item in items {
        let found = match item {
            XsdParticleItem::Any(any) => Some(found(any)),
            XsdParticleItem::Sequence(seq) => find_wildcard_in_items(&seq.particles)
                .map(|w| w.scaled(seq.min_occurs, seq.max_occurs, false)),
            XsdParticleItem::Choice(choice) => find_wildcard_in_items(&choice.particles).map(|w| {
                w.scaled(
                    choice.min_occurs,
                    choice.max_occurs,
                    choice.particles.len() > 1,
                )
            }),
            _ => None,
        };
        if let Some(w) = found {
            return Some(if optional {
                FoundWildcard { min: 0, ..w }
            } else {
                w
            });
        }
    }
    None
}

/// Checks cos-element-consistent: within one content model, element
/// declarations sharing a name must have the same type reference.
fn check_element_consistency(content: &ContentModel) -> Result<()> {
    let elements = match content {
        ContentModel::Sequence(e) | ContentModel::Choice(e) | ContentModel::All(e) => e,
        ContentModel::ComplexExtension { elements, .. } => elements,
        _ => return Ok(()),
    };
    let mut seen: std::collections::HashMap<&str, &Option<String>> =
        std::collections::HashMap::new();
    for elem in elements {
        if let Some(previous) = seen.get(elem.name.as_str()) {
            if **previous != elem.type_ref && previous.is_some() && elem.type_ref.is_some() {
                return Err(crate::schema::error::SchemaError::InvalidSchema {
                    message: format!(
                        "element '{}' is declared multiple times with different types",
                        elem.name
                    ),
                }
                .into());
            }
        } else {
            seen.insert(elem.name.as_str(), &elem.type_ref);
        }
    }
    Ok(())
}

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
                            if let Err(e) = crate::schema::xsd::regex_check::check_xsd_regex(p) {
                                return Err(crate::schema::error::SchemaError::InvalidSchema {
                                    message: format!("invalid pattern '{}': {}", p, e),
                                }
                                .into());
                            }
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

    /// Compiles an `xs:any` wildcard into its runtime representation.
    pub(crate) fn compile_wildcard(&self, any: &XsdAny) -> WildcardConstraint {
        WildcardConstraint {
            namespace: match &any.namespace {
                NamespaceConstraint::Any => WildcardNamespace::Any,
                NamespaceConstraint::Other => WildcardNamespace::Other,
                NamespaceConstraint::TargetNamespace => WildcardNamespace::List(vec![
                    self.current_target_ns.clone().unwrap_or_default(),
                ]),
                NamespaceConstraint::Local => WildcardNamespace::List(vec![String::new()]),
                NamespaceConstraint::List(uris) => WildcardNamespace::List(
                    uris.iter()
                        .map(|u| match u.as_str() {
                            "##targetNamespace" => {
                                self.current_target_ns.clone().unwrap_or_default()
                            }
                            "##local" => String::new(),
                            other => other.to_string(),
                        })
                        .collect(),
                ),
            },
            process_contents: match any.process_contents {
                ProcessContentsMode::Strict => ProcessContents::Strict,
                ProcessContentsMode::Lax => ProcessContents::Lax,
                ProcessContentsMode::Skip => ProcessContents::Skip,
            },
            min_occurs: match any.min_occurs {
                Occurs::Count(n) => n,
                Occurs::Unbounded => 0,
            },
            max_occurs: match any.max_occurs {
                Occurs::Count(n) => Some(n),
                Occurs::Unbounded => None,
            },
            target_namespace: self.current_target_ns.clone(),
        }
    }

    /// Compiles a complex type.
    pub(crate) fn compile_complex_type(&mut self, ct: &XsdComplexType) -> Result<TypeDef> {
        let name = ct.name.clone().unwrap_or_default();
        let mut compiled = ComplexType::new(&name);
        compiled.is_abstract = ct.is_abstract;
        compiled.mixed = ct.mixed;
        compiled.block =
            compile_block_set(ct.block.as_ref().or(self.current_block_default.as_ref()));

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
                    let base = self.resolve_qname(&ext.base);
                    // cos-all-limited: an xs:all group must constitute the
                    // whole content type; extending a base that already has
                    // element content with an xs:all is invalid.
                    if let Some(XsdParticle::All(all)) = &ext.particle
                        && !all.elements.is_empty()
                        && matches!(
                            self.type_cache.get(&base),
                            Some(TypeDef::Complex(b)) if !matches!(
                                b.content,
                                ContentModel::Empty | ContentModel::SimpleContent { .. }
                            )
                        )
                    {
                        return Err(crate::schema::error::SchemaError::InvalidSchema {
                            message: format!(
                                "cannot extend '{}' (which has element content) with an xs:all group",
                                base
                            ),
                        }
                        .into());
                    }
                    compiled.base_type = Some(base);
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

        // cos-element-consistent: element declarations with the same name in
        // one content model must have the same type.
        check_element_consistency(&compiled.content)?;

        // Record any element wildcard found in the content model
        compiled.wildcard = find_wildcard_in_content(&ct.content).map(|w| {
            let mut compiled_wc = self.compile_wildcard(w.any);
            compiled_wc.min_occurs = w.min;
            compiled_wc.max_occurs = w.max;
            compiled_wc
        });

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

        // Expand attribute group references (including groups referenced
        // from inside simpleContent/complexContent derivations).
        let mut group_refs: Vec<&QName> = ct.attribute_groups.iter().collect();
        let derivation_groups: Option<&[QName]> = match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => Some(&ext.attribute_groups),
                XsdSimpleContentDerivation::Restriction(r) => Some(&r.attribute_groups),
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => Some(&ext.attribute_groups),
                XsdComplexContentDerivation::Restriction(r) => Some(&r.attribute_groups),
            },
            _ => None,
        };
        group_refs.extend(derivation_groups.into_iter().flatten());
        let mut visited = std::collections::HashSet::new();
        let mut group_wildcard: Option<WildcardConstraint> = None;
        for ag_ref in group_refs {
            self.expand_attribute_group(
                ag_ref,
                &mut compiled.attributes,
                &mut group_wildcard,
                &mut visited,
            )?;
        }

        // Attribute wildcard: directly declared, from a derivation, or from
        // an expanded attribute group.
        let any_attribute = ct.any_attribute.as_ref().or(match &ct.content {
            XsdComplexContent::SimpleContent(sc) => match &sc.derivation {
                XsdSimpleContentDerivation::Extension(ext) => ext.any_attribute.as_ref(),
                XsdSimpleContentDerivation::Restriction(r) => r.any_attribute.as_ref(),
            },
            XsdComplexContent::ComplexContent(cc) => match &cc.derivation {
                XsdComplexContentDerivation::Extension(ext) => ext.any_attribute.as_ref(),
                XsdComplexContentDerivation::Restriction(r) => r.any_attribute.as_ref(),
            },
            _ => None,
        });
        compiled.attr_wildcard = any_attribute
            .map(|any| self.compile_wildcard(any))
            .or(group_wildcard);

        Ok(TypeDef::Complex(compiled))
    }

    /// Expands an attribute group reference into attribute definitions,
    /// following nested group references.
    fn expand_attribute_group(
        &mut self,
        ag_ref: &QName,
        out: &mut Vec<AttributeDef>,
        wildcard: &mut Option<WildcardConstraint>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        if !visited.insert(ag_ref.local.clone()) {
            return Ok(());
        }
        let ns = self.current_target_ns.clone().unwrap_or_default();
        let key = crate::schema::types::NsName::new(ns, ag_ref.local.clone());
        let group = match self.attribute_groups.get(&key) {
            Some(g) => g.clone(),
            None => {
                // Fall back to any namespace with the same local name.
                match self
                    .attribute_groups
                    .iter()
                    .find(|(k, _)| k.local_name == ag_ref.local)
                    .map(|(_, g)| g.clone())
                {
                    Some(g) => g,
                    None => {
                        // Unresolvable reference (e.g. xs:redefine, which is
                        // not supported): the attribute model is incomplete,
                        // so admit unknown attributes leniently instead of
                        // reporting false "not allowed" errors.
                        if wildcard.is_none() {
                            *wildcard = Some(WildcardConstraint {
                                namespace: WildcardNamespace::Any,
                                process_contents: ProcessContents::Lax,
                                min_occurs: 0,
                                max_occurs: None,
                                target_namespace: None,
                            });
                        }
                        return Ok(());
                    }
                }
            }
        };
        for attr in &group.attributes {
            out.push(self.compile_attribute(attr)?);
        }
        if wildcard.is_none() {
            *wildcard = group
                .any_attribute
                .as_ref()
                .map(|a| self.compile_wildcard(a));
        }
        for nested in &group.attribute_groups {
            self.expand_attribute_group(nested, out, wildcard, visited)?;
        }
        Ok(())
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
