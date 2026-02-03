//! XSD Compiler - transforms XSD AST into CompiledSchema.
//!
//! This module compiles parsed XSD schemas into the runtime validation
//! representation (CompiledSchema).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::error::Result;
use crate::schema::types::{
    AttributeDef, CompiledSchema, ComplexType, ContentModel, ContentModelType, ElementDef,
    FlattenedChildren, ProcessContents, SimpleType, TypeDef,
};

use super::types::*;

/// XSD Compiler that transforms AST into CompiledSchema.
pub struct XsdCompiler {
    /// Type cache for resolving references
    type_cache: HashMap<String, TypeDef>,
    /// Substitution group index (head -> members)
    substitution_groups: HashMap<String, Vec<String>>,
    /// Namespace bindings for resolving prefixes
    namespace_bindings: HashMap<String, String>,
    /// Current target namespace
    current_target_ns: Option<String>,
}

impl XsdCompiler {
    /// Creates a new compiler.
    pub fn new() -> Self {
        Self {
            type_cache: HashMap::new(),
            substitution_groups: HashMap::new(),
            namespace_bindings: HashMap::new(),
            current_target_ns: None,
        }
    }

    /// Compiles multiple XSD schemas into a single CompiledSchema.
    ///
    /// Schemas should be provided in dependency order (dependencies first).
    pub fn compile(&mut self, schemas: Vec<XsdSchema>) -> Result<CompiledSchema> {
        let mut result = CompiledSchema::new();

        // First pass: register all types for forward reference resolution
        for schema in &schemas {
            self.register_types(schema)?;
        }

        // Second pass: compile each schema
        for schema in schemas {
            self.compile_schema(schema, &mut result)?;
        }

        // Build substitution group index
        self.build_substitution_groups(&mut result);

        // Build performance optimization caches
        self.build_transitive_substitution_groups(&mut result);
        self.build_type_children_cache(&mut result);

        Ok(result)
    }

    /// Registers types from a schema for forward reference resolution.
    fn register_types(&mut self, schema: &XsdSchema) -> Result<()> {
        // Store namespace bindings
        for (prefix, uri) in &schema.namespace_bindings {
            self.namespace_bindings.insert(prefix.clone(), uri.clone());
        }

        // Register types with their qualified names
        let ns_prefix = schema.target_namespace.as_ref().and_then(|ns| {
            self.namespace_bindings
                .iter()
                .find(|(_, v)| *v == ns)
                .map(|(k, _)| k.clone())
        });

        for type_def in &schema.types {
            if let Some(name) = type_def.name() {
                let qname = match &ns_prefix {
                    Some(p) if !p.is_empty() => format!("{}:{}", p, name),
                    _ => name.to_string(),
                };

                // Pre-register as placeholder
                let placeholder = match type_def {
                    XsdTypeDef::Simple(_) => TypeDef::Simple(SimpleType::new(name)),
                    XsdTypeDef::Complex(_) => TypeDef::Complex(ComplexType::new(name)),
                };
                self.type_cache.insert(qname, placeholder);
            }
        }

        Ok(())
    }

    /// Compiles a single schema into the result.
    fn compile_schema(&mut self, schema: XsdSchema, result: &mut CompiledSchema) -> Result<()> {
        self.current_target_ns = schema.target_namespace.clone();

        // Set target namespace if this is the first schema with one
        if result.target_namespace.is_none() && schema.target_namespace.is_some() {
            result.target_namespace = schema.target_namespace.clone();
        }

        // Compile types
        for type_def in schema.types {
            let compiled = self.compile_type(&type_def)?;
            if let Some(name) = type_def.name() {
                result.types.insert(name.to_string(), compiled.clone());

                // Also update cache with full definition
                let qname = self.make_qname(name);
                self.type_cache.insert(qname, compiled);
            }
        }

        // Compile elements
        for element in schema.elements {
            let compiled = self.compile_element(&element)?;
            result.elements.insert(element.name.clone(), compiled);
        }

        // Compile top-level attributes
        for attr in schema.attributes {
            if let Some(name) = &attr.name {
                let compiled = self.compile_attribute(&attr)?;
                result.attributes.insert(name.clone(), compiled);
            }
        }

        Ok(())
    }

    /// Makes a qualified name using current namespace prefix.
    fn make_qname(&self, local: &str) -> String {
        if let Some(ns) = &self.current_target_ns {
            if let Some((prefix, _)) = self.namespace_bindings.iter().find(|(_, v)| *v == ns) {
                if !prefix.is_empty() {
                    return format!("{}:{}", prefix, local);
                }
            }
        }
        local.to_string()
    }

    /// Resolves a QName to its full qualified name.
    fn resolve_qname(&self, qname: &QName) -> String {
        qname.to_string_full()
    }

    /// Compiles a type definition.
    fn compile_type(&mut self, type_def: &XsdTypeDef) -> Result<TypeDef> {
        match type_def {
            XsdTypeDef::Simple(st) => self.compile_simple_type(st),
            XsdTypeDef::Complex(ct) => self.compile_complex_type(ct),
        }
    }

    /// Compiles a simple type.
    fn compile_simple_type(&mut self, st: &XsdSimpleType) -> Result<TypeDef> {
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
    fn compile_complex_type(&mut self, ct: &XsdComplexType) -> Result<TypeDef> {
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
    fn compile_complex_content(&mut self, content: &XsdComplexContent) -> Result<ContentModel> {
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

    /// Compiles a particle to a content model.
    fn compile_particle(&mut self, particle: &XsdParticle) -> Result<ContentModel> {
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
    fn compile_particle_to_elements(&mut self, particle: &XsdParticle) -> Result<Vec<ElementDef>> {
        match particle {
            XsdParticle::Sequence(seq) => self.compile_sequence(seq),
            XsdParticle::Choice(choice) => self.compile_choice(choice),
            XsdParticle::All(all) => self.compile_all(all),
            XsdParticle::GroupRef(_) => Ok(Vec::new()),
            XsdParticle::Any(_) => Ok(Vec::new()),
        }
    }

    /// Compiles a sequence.
    fn compile_sequence(&mut self, seq: &XsdSequence) -> Result<Vec<ElementDef>> {
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
    fn multiply_occurs(elem_max: Option<u32>, parent_max: Option<u32>) -> Option<u32> {
        match (elem_max, parent_max) {
            (None, _) | (_, None) => None, // unbounded
            (Some(a), Some(b)) => Some(a.saturating_mul(b)),
        }
    }

    /// Compiles a choice.
    fn compile_choice(&mut self, choice: &XsdChoice) -> Result<Vec<ElementDef>> {
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
    fn compile_all(&mut self, all: &XsdAll) -> Result<Vec<ElementDef>> {
        let mut elements = Vec::new();

        for elem in &all.elements {
            elements.push(self.compile_element(elem)?);
        }

        Ok(elements)
    }

    /// Compiles an element definition.
    fn compile_element(&mut self, elem: &XsdElement) -> Result<ElementDef> {
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

    /// Compiles an attribute definition.
    fn compile_attribute(&self, attr: &XsdAttribute) -> Result<AttributeDef> {
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

    /// Builds the substitution group index.
    fn build_substitution_groups(&mut self, schema: &mut CompiledSchema) {
        // Collect substitution group relationships
        for elem in schema.elements.values() {
            if let Some(sg_head) = &elem.substitution_group {
                self.substitution_groups
                    .entry(sg_head.clone())
                    .or_default()
                    .push(elem.name.clone());
            }
        }

        // Store in schema for validation use
        schema.substitution_groups = self.substitution_groups.clone();
    }

    /// Builds the transitive substitution groups cache.
    ///
    /// This pre-computes all transitive members for each substitution group head,
    /// so validation doesn't need to recurse at runtime.
    fn build_transitive_substitution_groups(&self, schema: &mut CompiledSchema) {
        // Build reverse lookup (member -> head)
        // Register both prefixed and non-prefixed versions for efficient lookup
        for (head, members) in &schema.substitution_groups {
            for member in members {
                // Register with original name
                schema
                    .substitution_group_heads
                    .insert(member.clone(), head.clone());

                // Also register with local name (without prefix) for faster lookup
                if let Some((_prefix, local)) = member.split_once(':') {
                    schema
                        .substitution_group_heads
                        .entry(local.to_string())
                        .or_insert_with(|| head.clone());
                }
            }
        }

        // Build transitive closure for each head
        // Register both prefixed and non-prefixed versions for efficient lookup
        for head in schema.substitution_groups.keys() {
            let mut all_members = Vec::new();
            let mut visited = HashSet::new();
            self.collect_transitive_substitution_members(
                head,
                &schema.substitution_groups,
                &mut all_members,
                &mut visited,
            );
            let members_arc = Arc::new(all_members);

            // Register with original name
            schema
                .transitive_substitution_groups
                .insert(head.clone(), Arc::clone(&members_arc));

            // Also register with local name (without prefix) for faster lookup
            if let Some((_prefix, local)) = head.split_once(':') {
                schema
                    .transitive_substitution_groups
                    .entry(local.to_string())
                    .or_insert_with(|| Arc::clone(&members_arc));
            }
        }
    }

    /// Helper to recursively collect substitution group members.
    fn collect_transitive_substitution_members(
        &self,
        head_name: &str,
        groups: &HashMap<String, Vec<String>>,
        members: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) {
        if visited.contains(head_name) {
            return;
        }
        visited.insert(head_name.to_string());

        // Try multiple name variants for namespace prefix handling
        let mut names_to_try = vec![head_name.to_string()];

        if let Some((_prefix, local)) = head_name.split_once(':') {
            names_to_try.push(local.to_string());
        } else {
            // No prefix -> try with common prefixes from schema
            for key in groups.keys() {
                if let Some((_prefix, local)) = key.split_once(':') {
                    if local == head_name {
                        names_to_try.push(key.clone());
                    }
                }
            }
        }

        for name in names_to_try {
            if let Some(direct_members) = groups.get(&name) {
                for member in direct_members {
                    if !members.contains(member) {
                        members.push(member.clone());
                    }
                    // Recursively collect this member's members
                    self.collect_transitive_substitution_members(member, groups, members, visited);
                }
            }
        }
    }

    /// Builds the type children cache.
    ///
    /// This pre-computes the flattened child element constraints for each complex type,
    /// including elements inherited through type extension.
    ///
    /// Types are stored with both their local name AND qualified names (with common prefixes)
    /// to ensure fast cache hits regardless of how the type is referenced.
    fn build_type_children_cache(&self, schema: &mut CompiledSchema) {
        // Collect type names first to avoid borrowing issues
        let type_names: Vec<String> = schema.types.keys().cloned().collect();

        // Build cache for main schema types
        for type_name in type_names {
            if let Some(TypeDef::Complex(complex)) = schema.types.get(&type_name) {
                let flattened = Arc::new(self.flatten_type_children(complex, schema));

                // Insert with local name
                schema
                    .type_children_cache
                    .insert(type_name.clone(), Arc::clone(&flattened));

                // Also insert with common namespace prefixes to avoid split_once at runtime
                for prefix in &[
                    "gml", "core", "xs", "xsd", "bldg", "dem", "tran", "urf", "luse", "fld", "uro",
                    "gen",
                ] {
                    let qualified = format!("{}:{}", prefix, type_name);
                    schema
                        .type_children_cache
                        .insert(qualified, Arc::clone(&flattened));
                }
            }
        }

        // Build cache for imported schema types
        // We need to collect these separately and then add them
        let import_types: Vec<(String, FlattenedChildren)> = schema
            .imports
            .values()
            .flat_map(|imported| {
                imported.types.iter().filter_map(|(type_name, type_def)| {
                    if let TypeDef::Complex(complex) = type_def {
                        let flattened = self.flatten_type_children(complex, schema);
                        Some((type_name.clone(), flattened))
                    } else {
                        None
                    }
                })
            })
            .collect();

        for (type_name, flattened) in import_types {
            let flattened = Arc::new(flattened);
            schema
                .type_children_cache
                .insert(type_name.clone(), Arc::clone(&flattened));

            // Also insert with common namespace prefixes
            for prefix in &[
                "gml", "core", "xs", "xsd", "bldg", "dem", "tran", "urf", "luse", "fld", "uro",
                "gen",
            ] {
                let qualified = format!("{}:{}", prefix, type_name);
                schema
                    .type_children_cache
                    .insert(qualified, Arc::clone(&flattened));
            }
        }
    }

    /// Flattens the child element constraints for a complex type.
    fn flatten_type_children(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
    ) -> FlattenedChildren {
        let mut visited = HashSet::new();
        let elements = self.collect_elements_with_inheritance(complex, schema, &mut visited);

        // Determine content model type
        let content_model_type = match &complex.content {
            ContentModel::Sequence(_) => ContentModelType::Sequence,
            ContentModel::Choice(_) => ContentModelType::Choice,
            ContentModel::All(_) => ContentModelType::All,
            ContentModel::ComplexExtension { .. } => ContentModelType::Sequence,
            ContentModel::Empty => ContentModelType::Empty,
            ContentModel::SimpleContent { .. } => ContentModelType::Empty,
            ContentModel::Any { .. } => ContentModelType::Sequence,
        };

        let mut flattened = FlattenedChildren::with_content_model(content_model_type);
        for elem in elements {
            flattened
                .constraints
                .insert(elem.name.clone(), (elem.min_occurs, elem.max_occurs));
        }

        flattened
    }

    /// Collects all child elements from a complex type, including inherited elements.
    fn collect_elements_with_inheritance(
        &self,
        complex: &ComplexType,
        schema: &CompiledSchema,
        visited: &mut HashSet<String>,
    ) -> Vec<ElementDef> {
        let mut elements = Vec::new();

        match &complex.content {
            ContentModel::Sequence(elems)
            | ContentModel::Choice(elems)
            | ContentModel::All(elems) => {
                elements.extend(elems.iter().cloned());
            }
            ContentModel::ComplexExtension {
                base_type,
                elements: ext_elements,
            } => {
                // First, get elements from the base type (inherited elements)
                if !visited.contains(base_type.as_str()) {
                    visited.insert(base_type.clone());
                    if let Some(TypeDef::Complex(base_complex)) =
                        schema.get_type(base_type.as_str())
                    {
                        let base_elements =
                            self.collect_elements_with_inheritance(base_complex, schema, visited);
                        elements.extend(base_elements);
                    }
                }
                // Then add the extension's own elements
                elements.extend(ext_elements.iter().cloned());
            }
            _ => {}
        }

        elements
    }

    /// Resolves a type reference to its definition.
    pub fn resolve_type(&self, type_ref: &str) -> Option<&TypeDef> {
        self.type_cache.get(type_ref)
    }
}

impl Default for XsdCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiles XSD AST schemas into a CompiledSchema.
pub fn compile_schemas(schemas: Vec<XsdSchema>) -> Result<CompiledSchema> {
    let mut compiler = XsdCompiler::new();
    compiler.compile(schemas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::xsd::parser::parse_xsd_ast;

    #[test]
    fn test_compile_simple_schema() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/test">
            <xs:element name="root" type="xs:string"/>
        </xs:schema>"#;

        let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
        let compiled = compile_schemas(vec![ast]).unwrap();

        assert_eq!(
            compiled.target_namespace,
            Some("http://example.com/test".to_string())
        );
        assert_eq!(compiled.elements.len(), 1);
        assert!(compiled.elements.contains_key("root"));
    }

    #[test]
    fn test_compile_complex_type() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="PersonType">
                <xs:sequence>
                    <xs:element name="name" type="xs:string"/>
                    <xs:element name="age" type="xs:integer" minOccurs="0"/>
                </xs:sequence>
            </xs:complexType>
        </xs:schema>"#;

        let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
        let compiled = compile_schemas(vec![ast]).unwrap();

        assert!(compiled.types.contains_key("PersonType"));
        if let Some(TypeDef::Complex(ct)) = compiled.types.get("PersonType") {
            if let ContentModel::Sequence(elems) = &ct.content {
                assert_eq!(elems.len(), 2);
                assert_eq!(elems[0].name, "name");
                assert_eq!(elems[1].name, "age");
                assert_eq!(elems[1].min_occurs, 0);
            } else {
                panic!("Expected sequence content");
            }
        } else {
            panic!("Expected complex type");
        }
    }

    #[test]
    fn test_compile_simple_type_enumeration() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="StatusType">
                <xs:restriction base="xs:string">
                    <xs:enumeration value="active"/>
                    <xs:enumeration value="inactive"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
        let compiled = compile_schemas(vec![ast]).unwrap();

        if let Some(TypeDef::Simple(st)) = compiled.types.get("StatusType") {
            assert_eq!(st.enumeration.len(), 2);
            assert!(st.enumeration.contains(&"active".to_string()));
            assert!(st.enumeration.contains(&"inactive".to_string()));
        } else {
            panic!("Expected simple type");
        }
    }

    #[test]
    fn test_compile_extension() {
        let xsd = r#"<?xml version="1.0"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="ExtendedType">
                <xs:complexContent>
                    <xs:extension base="BaseType">
                        <xs:sequence>
                            <xs:element name="extra" type="xs:string"/>
                        </xs:sequence>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;

        let ast = parse_xsd_ast(xsd.as_bytes()).unwrap();
        let compiled = compile_schemas(vec![ast]).unwrap();

        if let Some(TypeDef::Complex(ct)) = compiled.types.get("ExtendedType") {
            if let ContentModel::ComplexExtension {
                base_type,
                elements,
            } = &ct.content
            {
                assert_eq!(base_type, "BaseType");
                assert_eq!(elements.len(), 1);
                assert_eq!(elements[0].name, "extra");
            } else {
                panic!("Expected complex extension");
            }
        } else {
            panic!("Expected complex type");
        }
    }
}
