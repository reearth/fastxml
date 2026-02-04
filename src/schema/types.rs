//! XSD schema type definitions.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

/// Content model type for an element (used in caches).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentModelType {
    /// Sequence - all elements in order, each with their own min/max occurs
    #[default]
    Sequence,
    /// Choice - exactly one of the elements must be present
    Choice,
    /// All - all elements must be present, but in any order
    All,
    /// Empty - no child elements allowed
    Empty,
}

/// Pre-computed child element constraints for a type.
///
/// This struct caches the flattened inheritance chain for a complex type,
/// eliminating the need to traverse the inheritance hierarchy at validation time.
#[derive(Debug, Clone)]
pub struct FlattenedChildren {
    /// Child element constraints (name -> (min_occurs, max_occurs))
    pub constraints: HashMap<String, (u32, Option<u32>)>,
    /// Content model type
    pub content_model_type: ContentModelType,
    /// Ordered element names for sequence validation.
    /// Using Arc<[String]> to make cloning cheap (pointer copy instead of deep clone).
    pub ordered_elements: Arc<[String]>,
}

impl FlattenedChildren {
    /// Creates a new empty FlattenedChildren.
    pub fn new() -> Self {
        Self {
            constraints: HashMap::new(),
            content_model_type: ContentModelType::Empty,
            ordered_elements: Arc::from([]),
        }
    }

    /// Creates a FlattenedChildren with the given content model type.
    pub fn with_content_model(content_model_type: ContentModelType) -> Self {
        Self {
            constraints: HashMap::new(),
            content_model_type,
            ordered_elements: Arc::from([]),
        }
    }
}

impl Default for FlattenedChildren {
    fn default() -> Self {
        Self::new()
    }
}

/// A compiled XSD schema.
#[derive(Debug, Clone)]
pub struct CompiledSchema {
    /// Target namespace
    pub target_namespace: Option<String>,
    /// Element definitions
    pub elements: IndexMap<String, ElementDef>,
    /// Type definitions
    pub types: IndexMap<String, TypeDef>,
    /// Attribute definitions
    pub attributes: IndexMap<String, AttributeDef>,
    /// Imported schemas (namespace -> schema)
    pub imports: HashMap<String, CompiledSchema>,
    /// Substitution groups (head element name -> list of substitute element names)
    pub substitution_groups: HashMap<String, Vec<String>>,

    // === Namespace resolution ===
    /// Namespace URI to prefix mapping (uri -> prefix).
    /// Used to resolve element lookups when XML uses different prefix than schema.
    pub namespace_prefixes: HashMap<String, String>,

    // === Performance optimization caches ===
    /// Pre-computed flattened child elements per type (type_name -> `Arc<FlattenedChildren>`).
    ///
    /// This cache stores the inheritance-flattened child element constraints for each type,
    /// eliminating the need to traverse the type hierarchy at validation time.
    pub type_children_cache: HashMap<String, Arc<FlattenedChildren>>,

    /// Pre-computed transitive substitution groups (head -> all members).
    ///
    /// This cache stores all transitive members of each substitution group head,
    /// including indirect members (e.g., if A has member B, and B has member C,
    /// this stores [B, C] for A).
    pub transitive_substitution_groups: HashMap<String, Arc<Vec<String>>>,

    /// Reverse lookup: substitution group member -> head element.
    ///
    /// This allows O(1) lookup of which substitution group head an element belongs to.
    pub substitution_group_heads: HashMap<String, String>,
}

impl CompiledSchema {
    /// Creates a new empty schema.
    pub fn new() -> Self {
        Self {
            target_namespace: None,
            elements: IndexMap::new(),
            types: IndexMap::new(),
            attributes: IndexMap::new(),
            imports: HashMap::new(),
            substitution_groups: HashMap::new(),
            // Namespace resolution
            namespace_prefixes: HashMap::new(),
            // Performance optimization caches
            type_children_cache: HashMap::new(),
            transitive_substitution_groups: HashMap::new(),
            substitution_group_heads: HashMap::new(),
        }
    }

    /// Creates a schema with a target namespace.
    pub fn with_namespace(namespace: impl Into<String>) -> Self {
        Self {
            target_namespace: Some(namespace.into()),
            ..Self::new()
        }
    }

    /// Looks up an element definition by qualified name.
    ///
    /// This method tries multiple strategies:
    /// 1. Direct lookup with the full qname (e.g., "dem:ReliefFeature")
    /// 2. If qname has a prefix, extract local name and try:
    ///    a. The local name in imported schemas
    ///    b. The local name in the main elements map (for merged schemas)
    pub fn get_element(&self, qname: &str) -> Option<&ElementDef> {
        // Try with full qname first
        if let Some(elem) = self.elements.get(qname) {
            return Some(elem);
        }

        // If qname has a prefix, try the local name
        if let Some((_prefix, local)) = qname.split_once(':') {
            // Try imported schemas first
            for schema in self.imports.values() {
                if let Some(elem) = schema.elements.get(local) {
                    return Some(elem);
                }
            }
            // Also try local name in main map (for merged schemas)
            if let Some(elem) = self.elements.get(local) {
                return Some(elem);
            }
        }

        None
    }

    /// Looks up an element definition by namespace URI and local name.
    ///
    /// This method resolves the namespace URI to the prefix used in the schema,
    /// then constructs the qualified name and looks it up. This allows finding
    /// elements even when the XML uses a different prefix than the schema.
    ///
    /// Example: If schema uses `tran:Road` but XML uses `tr:Road` (same namespace),
    /// calling `get_element_by_ns("http://...transportation...", "Road")` will
    /// correctly find the element.
    pub fn get_element_by_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&ElementDef> {
        // First, try to find the schema's prefix for this namespace URI
        if let Some(prefix) = self.namespace_prefixes.get(namespace_uri) {
            // Construct the qname using the schema's prefix
            let qname = if prefix.is_empty() {
                local_name.to_string()
            } else {
                format!("{}:{}", prefix, local_name)
            };
            if let Some(elem) = self.elements.get(&qname) {
                return Some(elem);
            }
        }

        // Try local name directly (for schemas without prefix)
        if let Some(elem) = self.elements.get(local_name) {
            return Some(elem);
        }

        // Try all elements with matching local name (brute force fallback)
        for (key, elem) in &self.elements {
            if let Some((_prefix, local)) = key.split_once(':') {
                if local == local_name {
                    return Some(elem);
                }
            }
        }

        None
    }

    /// Looks up a type definition by qualified name.
    ///
    /// This method tries multiple strategies:
    /// 1. Direct lookup with the full qname (e.g., "core:AbstractCityObjectType")
    /// 2. If qname has a prefix, extract local name and try:
    ///    a. The local name in imported schemas
    ///    b. The local name in the main types map (for merged schemas)
    pub fn get_type(&self, qname: &str) -> Option<&TypeDef> {
        // Try with full qname first
        if let Some(typ) = self.types.get(qname) {
            return Some(typ);
        }

        // If qname has a prefix, try the local name
        if let Some((_prefix, local)) = qname.split_once(':') {
            // Try imported schemas first
            for schema in self.imports.values() {
                if let Some(typ) = schema.types.get(local) {
                    return Some(typ);
                }
            }
            // Also try local name in main map (for merged schemas)
            if let Some(typ) = self.types.get(local) {
                return Some(typ);
            }
        }

        None
    }
}

impl Default for CompiledSchema {
    fn default() -> Self {
        Self::new()
    }
}

/// An element definition.
#[derive(Debug, Clone)]
pub struct ElementDef {
    /// Element name
    pub name: String,
    /// Type reference (name of a type definition)
    pub type_ref: Option<String>,
    /// Inline type definition
    pub inline_type: Option<TypeDef>,
    /// Minimum occurrences
    pub min_occurs: u32,
    /// Maximum occurrences (None = unbounded)
    pub max_occurs: Option<u32>,
    /// Whether this element is abstract
    pub is_abstract: bool,
    /// Substitution group head
    pub substitution_group: Option<String>,
    /// Whether the element is nillable
    pub nillable: bool,
    /// Identity constraints (unique, key, keyref)
    pub constraints: Vec<CompiledConstraint>,
}

impl ElementDef {
    /// Creates a new element definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_ref: None,
            inline_type: None,
            min_occurs: 1,
            max_occurs: Some(1),
            is_abstract: false,
            substitution_group: None,
            nillable: false,
            constraints: Vec::new(),
        }
    }

    /// Sets the type reference.
    pub fn with_type(mut self, type_ref: impl Into<String>) -> Self {
        self.type_ref = Some(type_ref.into());
        self
    }

    /// Sets the occurrence bounds.
    pub fn with_occurs(mut self, min: u32, max: Option<u32>) -> Self {
        self.min_occurs = min;
        self.max_occurs = max;
        self
    }

    /// Makes this element optional (minOccurs=0).
    pub fn optional(mut self) -> Self {
        self.min_occurs = 0;
        self
    }

    /// Makes this element repeatable (maxOccurs=unbounded).
    pub fn unbounded(mut self) -> Self {
        self.max_occurs = None;
        self
    }
}

/// A type definition.
#[derive(Debug, Clone)]
pub enum TypeDef {
    /// Simple type (string, integer, etc.)
    Simple(SimpleType),
    /// Complex type (elements, attributes)
    Complex(ComplexType),
}

/// A simple type definition.
#[derive(Debug, Clone)]
pub struct SimpleType {
    /// Type name
    pub name: String,
    /// Base type (restriction or extension)
    pub base_type: Option<String>,
    /// Enumeration values
    pub enumeration: Vec<String>,
    /// Pattern restriction
    pub pattern: Option<String>,
    /// Min length restriction
    pub min_length: Option<u32>,
    /// Max length restriction
    pub max_length: Option<u32>,
    /// Minimum value (inclusive)
    pub min_inclusive: Option<String>,
    /// Maximum value (inclusive)
    pub max_inclusive: Option<String>,
}

impl SimpleType {
    /// Creates a new simple type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            enumeration: Vec::new(),
            pattern: None,
            min_length: None,
            max_length: None,
            min_inclusive: None,
            max_inclusive: None,
        }
    }

    /// Creates a string type.
    pub fn string() -> Self {
        Self::new("string").with_base("xs:string")
    }

    /// Creates an integer type.
    pub fn integer() -> Self {
        Self::new("integer").with_base("xs:integer")
    }

    /// Sets the base type.
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base_type = Some(base.into());
        self
    }
}

/// A complex type definition.
#[derive(Debug, Clone)]
pub struct ComplexType {
    /// Type name
    pub name: String,
    /// Base type (for extension or restriction)
    pub base_type: Option<String>,
    /// Content model
    pub content: ContentModel,
    /// Attribute definitions
    pub attributes: Vec<AttributeDef>,
    /// Whether this type is abstract
    pub is_abstract: bool,
    /// Whether content is mixed (text + elements)
    pub mixed: bool,
}

impl ComplexType {
    /// Creates a new complex type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            content: ContentModel::Empty,
            attributes: Vec::new(),
            is_abstract: false,
            mixed: false,
        }
    }

    /// Creates a complex type with sequence content.
    pub fn sequence(name: impl Into<String>, elements: Vec<ElementDef>) -> Self {
        Self {
            name: name.into(),
            base_type: None,
            content: ContentModel::Sequence(elements),
            attributes: Vec::new(),
            is_abstract: false,
            mixed: false,
        }
    }
}

/// Content model for complex types.
#[derive(Debug, Clone)]
pub enum ContentModel {
    /// Empty content
    Empty,
    /// Sequence of elements (in order)
    Sequence(Vec<ElementDef>),
    /// Choice of elements (one of)
    Choice(Vec<ElementDef>),
    /// All (unordered)
    All(Vec<ElementDef>),
    /// Simple content (text with optional extension)
    SimpleContent {
        /// Base type for the content
        base_type: String,
    },
    /// Complex content extension
    ComplexExtension {
        /// Base type being extended
        base_type: String,
        /// Additional child elements
        elements: Vec<ElementDef>,
    },
    /// Any content (wildcard)
    Any {
        /// Target namespace for wildcard
        namespace: Option<String>,
        /// How to process the content
        process_contents: ProcessContents,
    },
}

/// How to process wildcard content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessContents {
    /// Strict - must validate
    Strict,
    /// Lax - validate if schema available
    Lax,
    /// Skip - don't validate
    Skip,
}

/// An attribute definition.
#[derive(Debug, Clone)]
pub struct AttributeDef {
    /// Attribute name
    pub name: String,
    /// Type reference
    pub type_ref: Option<String>,
    /// Whether the attribute is required
    pub required: bool,
    /// Default value
    pub default: Option<String>,
    /// Fixed value
    pub fixed: Option<String>,
}

impl AttributeDef {
    /// Creates a new attribute definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_ref: None,
            required: false,
            default: None,
            fixed: None,
        }
    }

    /// Creates a required attribute.
    pub fn required(name: impl Into<String>) -> Self {
        Self {
            required: true,
            ..Self::new(name)
        }
    }

    /// Sets the type reference.
    pub fn with_type(mut self, type_ref: impl Into<String>) -> Self {
        self.type_ref = Some(type_ref.into());
        self
    }

    /// Sets a default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

/// Type of compiled identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// A compiled identity constraint ready for validation.
#[derive(Debug, Clone)]
pub struct CompiledConstraint {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: CompiledConstraintType,
    /// XPath selector expression (for selecting scope)
    pub selector_xpath: String,
    /// XPath field expressions (for selecting key values)
    pub field_xpaths: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<String>,
}

impl CompiledConstraint {
    /// Creates a new compiled unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::Unique,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new compiled key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::Key,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new compiled keyref constraint.
    pub fn keyref(
        name: impl Into<String>,
        selector: impl Into<String>,
        refer: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            constraint_type: CompiledConstraintType::KeyRef,
            selector_xpath: selector.into(),
            field_xpaths: Vec::new(),
            refer: Some(refer.into()),
        }
    }

    /// Adds a field XPath expression.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field_xpaths.push(field.into());
        self
    }

    /// Adds multiple field XPath expressions.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.field_xpaths.extend(fields.into_iter().map(Into::into));
        self
    }
}

/// Built-in XSD types.
pub mod builtin {
    /// Built-in string type name.
    pub const STRING: &str = "xs:string";
    /// Built-in integer type name.
    pub const INTEGER: &str = "xs:integer";
    /// Built-in decimal type name.
    pub const DECIMAL: &str = "xs:decimal";
    /// Built-in boolean type name.
    pub const BOOLEAN: &str = "xs:boolean";
    /// Built-in date type name.
    pub const DATE: &str = "xs:date";
    /// Built-in dateTime type name.
    pub const DATE_TIME: &str = "xs:dateTime";
    /// Built-in double type name.
    pub const DOUBLE: &str = "xs:double";
    /// Built-in float type name.
    pub const FLOAT: &str = "xs:float";
    /// Built-in anyURI type name.
    pub const ANY_URI: &str = "xs:anyURI";
    /// Built-in ID type name.
    pub const ID: &str = "xs:ID";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_element_with_local_name() {
        let mut schema = CompiledSchema::new();
        schema.elements.insert(
            "ReliefFeature".to_string(),
            ElementDef::new("ReliefFeature"),
        );

        // Local name lookup should work
        assert!(schema.get_element("ReliefFeature").is_some());
    }

    #[test]
    fn test_get_element_with_qualified_name() {
        let mut schema = CompiledSchema::new();
        schema.elements.insert(
            "ReliefFeature".to_string(),
            ElementDef::new("ReliefFeature"),
        );

        // Qualified name lookup should fall back to local name
        assert!(
            schema.get_element("dem:ReliefFeature").is_some(),
            "Should find 'ReliefFeature' when looking up 'dem:ReliefFeature'"
        );
    }

    #[test]
    fn test_get_type_with_local_name() {
        let mut schema = CompiledSchema::new();
        schema.types.insert(
            "AbstractCityObjectType".to_string(),
            TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
        );

        // Local name lookup should work
        assert!(schema.get_type("AbstractCityObjectType").is_some());
    }

    #[test]
    fn test_get_type_with_qualified_name() {
        let mut schema = CompiledSchema::new();
        schema.types.insert(
            "AbstractCityObjectType".to_string(),
            TypeDef::Complex(ComplexType::new("AbstractCityObjectType")),
        );

        // Qualified name lookup should fall back to local name
        assert!(
            schema.get_type("core:AbstractCityObjectType").is_some(),
            "Should find 'AbstractCityObjectType' when looking up 'core:AbstractCityObjectType'"
        );
    }

    #[test]
    fn test_get_type_not_found() {
        let schema = CompiledSchema::new();
        assert!(schema.get_type("NonExistentType").is_none());
        assert!(schema.get_type("prefix:NonExistentType").is_none());
    }

    #[test]
    fn test_get_element_not_found() {
        let schema = CompiledSchema::new();
        assert!(schema.get_element("NonExistentElement").is_none());
        assert!(schema.get_element("prefix:NonExistentElement").is_none());
    }
}
