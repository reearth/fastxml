//! XSD Abstract Syntax Tree (AST) types.
//!
//! These types represent the intermediate representation of an XSD schema
//! after parsing but before compilation into a CompiledSchema.

use std::collections::HashMap;

/// Qualified name with optional namespace prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    /// Namespace prefix (if any)
    pub prefix: Option<String>,
    /// Local name
    pub local: String,
}

impl QName {
    /// Creates a new QName with just a local name.
    pub fn new(local: impl Into<String>) -> Self {
        Self {
            prefix: None,
            local: local.into(),
        }
    }

    /// Creates a new QName with prefix and local name.
    pub fn with_prefix(prefix: impl Into<String>, local: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            local: local.into(),
        }
    }

    /// Parses a QName from a string like "prefix:local" or "local".
    pub fn parse(s: &str) -> Self {
        if let Some((prefix, local)) = s.split_once(':') {
            Self::with_prefix(prefix, local)
        } else {
            Self::new(s)
        }
    }

    /// Returns the full qualified name as a string.
    pub fn to_string_full(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{}:{}", p, self.local),
            None => self.local.clone(),
        }
    }
}

impl std::fmt::Display for QName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.prefix {
            Some(p) => write!(f, "{}:{}", p, self.local),
            None => write!(f, "{}", self.local),
        }
    }
}

/// Occurrence bounds for elements and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occurs {
    /// A specific count
    Count(u32),
    /// Unbounded (infinite)
    Unbounded,
}

impl Default for Occurs {
    fn default() -> Self {
        Occurs::Count(1)
    }
}

impl Occurs {
    /// Returns true if this represents unbounded.
    pub fn is_unbounded(&self) -> bool {
        matches!(self, Occurs::Unbounded)
    }

    /// Converts to an `Option<u32>` where None means unbounded.
    pub fn to_option(&self) -> Option<u32> {
        match self {
            Occurs::Count(n) => Some(*n),
            Occurs::Unbounded => None,
        }
    }

    /// Parses from a string, handling "unbounded".
    /// Returns an error for invalid values (non-numeric, negative in string form).
    pub fn parse(s: &str) -> Result<Self, String> {
        if s == "unbounded" {
            Ok(Occurs::Unbounded)
        } else {
            // Check for negative values (string starts with '-')
            if s.starts_with('-') {
                return Err(format!(
                    "invalid occurs value '{}': negative values not allowed",
                    s
                ));
            }
            s.parse::<u32>().map(Occurs::Count).map_err(|_| {
                format!(
                    "invalid occurs value '{}': must be a non-negative integer or 'unbounded'",
                    s
                )
            })
        }
    }
}

/// A parsed XSD schema.
#[derive(Debug, Clone, Default)]
pub struct XsdSchema {
    /// Target namespace for this schema
    pub target_namespace: Option<String>,
    /// Element form default (qualified/unqualified)
    pub element_form_default: FormDefault,
    /// Attribute form default (qualified/unqualified)
    pub attribute_form_default: FormDefault,
    /// Schema version
    pub version: Option<String>,
    /// Import declarations
    pub imports: Vec<XsdImport>,
    /// Include declarations
    pub includes: Vec<XsdInclude>,
    /// Redefine declarations
    pub redefines: Vec<XsdRedefine>,
    /// Top-level element declarations
    pub elements: Vec<XsdElement>,
    /// Type definitions (simple and complex)
    pub types: Vec<XsdTypeDef>,
    /// Top-level attribute declarations
    pub attributes: Vec<XsdAttribute>,
    /// Attribute group definitions
    pub attribute_groups: Vec<XsdAttributeGroup>,
    /// Model group definitions
    pub groups: Vec<XsdGroup>,
    /// Namespace bindings (prefix -> URI)
    pub namespace_bindings: HashMap<String, String>,
}

impl XsdSchema {
    /// Creates a new empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the target namespace.
    pub fn with_target_namespace(mut self, ns: impl Into<String>) -> Self {
        self.target_namespace = Some(ns.into());
        self
    }
}

/// Form default for elements/attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormDefault {
    /// Elements/attributes are qualified by default
    Qualified,
    /// Elements/attributes are unqualified by default
    #[default]
    Unqualified,
}

/// Import declaration.
#[derive(Debug, Clone)]
pub struct XsdImport {
    /// Namespace being imported
    pub namespace: Option<String>,
    /// Schema location URL
    pub schema_location: Option<String>,
}

/// Include declaration.
#[derive(Debug, Clone)]
pub struct XsdInclude {
    /// Schema location URL
    pub schema_location: String,
}

/// Redefine declaration.
///
/// Allows modification of schema components from an included schema.
#[derive(Debug, Clone)]
pub struct XsdRedefine {
    /// Schema location URL
    pub schema_location: String,
    /// Redefined simple types
    pub simple_types: Vec<XsdSimpleType>,
    /// Redefined complex types
    pub complex_types: Vec<XsdComplexType>,
    /// Redefined model groups
    pub groups: Vec<XsdGroup>,
    /// Redefined attribute groups
    pub attribute_groups: Vec<XsdAttributeGroup>,
}

impl XsdRedefine {
    /// Creates a new redefine declaration.
    pub fn new(schema_location: impl Into<String>) -> Self {
        Self {
            schema_location: schema_location.into(),
            simple_types: Vec::new(),
            complex_types: Vec::new(),
            groups: Vec::new(),
            attribute_groups: Vec::new(),
        }
    }
}

/// Type of identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XsdConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// Identity constraint definition (unique, key, keyref).
#[derive(Debug, Clone)]
pub struct XsdIdentityConstraint {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: XsdConstraintType,
    /// XPath selector expression
    pub selector: String,
    /// XPath field expressions (one or more for composite keys)
    pub fields: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<QName>,
}

impl XsdIdentityConstraint {
    /// Creates a new unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::Unique,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::Key,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new keyref constraint.
    pub fn keyref(name: impl Into<String>, selector: impl Into<String>, refer: QName) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::KeyRef,
            selector: selector.into(),
            fields: Vec::new(),
            refer: Some(refer),
        }
    }

    /// Adds a field expression.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.fields.push(field.into());
        self
    }

    /// Adds multiple field expressions.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.fields.extend(fields.into_iter().map(Into::into));
        self
    }
}

/// Element declaration.
#[derive(Debug, Clone)]
pub struct XsdElement {
    /// Element name
    pub name: String,
    /// Type reference (qualified name)
    pub type_ref: Option<QName>,
    /// Inline type definition
    pub inline_type: Option<Box<XsdTypeDef>>,
    /// Reference to another element
    pub ref_: Option<QName>,
    /// Minimum occurrences
    pub min_occurs: Occurs,
    /// Maximum occurrences
    pub max_occurs: Occurs,
    /// Whether this element is abstract
    pub is_abstract: bool,
    /// Substitution group head
    pub substitution_group: Option<QName>,
    /// Whether the element is nillable
    pub nillable: bool,
    /// Default value
    pub default: Option<String>,
    /// Fixed value
    pub fixed: Option<String>,
    /// Form (qualified/unqualified)
    pub form: Option<FormDefault>,
    /// Identity constraints (unique, key, keyref)
    pub identity_constraints: Vec<XsdIdentityConstraint>,
}

impl XsdElement {
    /// Creates a new element with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_ref: None,
            inline_type: None,
            ref_: None,
            min_occurs: Occurs::Count(1),
            max_occurs: Occurs::Count(1),
            is_abstract: false,
            substitution_group: None,
            nillable: false,
            default: None,
            fixed: None,
            form: None,
            identity_constraints: Vec::new(),
        }
    }

    /// Creates an element reference.
    pub fn ref_(ref_name: QName) -> Self {
        Self {
            name: String::new(),
            type_ref: None,
            inline_type: None,
            ref_: Some(ref_name),
            min_occurs: Occurs::Count(1),
            max_occurs: Occurs::Count(1),
            is_abstract: false,
            substitution_group: None,
            nillable: false,
            default: None,
            fixed: None,
            form: None,
            identity_constraints: Vec::new(),
        }
    }
}

/// Type definition (simple or complex).
#[derive(Debug, Clone)]
pub enum XsdTypeDef {
    /// Simple type
    Simple(XsdSimpleType),
    /// Complex type
    Complex(XsdComplexType),
}

impl XsdTypeDef {
    /// Gets the name of the type.
    pub fn name(&self) -> Option<&str> {
        match self {
            XsdTypeDef::Simple(s) => s.name.as_deref(),
            XsdTypeDef::Complex(c) => c.name.as_deref(),
        }
    }

    /// Returns true if this is a simple type.
    pub fn is_simple(&self) -> bool {
        matches!(self, XsdTypeDef::Simple(_))
    }

    /// Returns true if this is a complex type.
    pub fn is_complex(&self) -> bool {
        matches!(self, XsdTypeDef::Complex(_))
    }
}

/// Simple type definition.
#[derive(Debug, Clone)]
pub struct XsdSimpleType {
    /// Type name (None for anonymous types)
    pub name: Option<String>,
    /// Content definition
    pub content: XsdSimpleTypeContent,
}

impl XsdSimpleType {
    /// Creates a new named simple type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            content: XsdSimpleTypeContent::Restriction(XsdSimpleRestriction::default()),
        }
    }

    /// Creates an anonymous simple type.
    pub fn anonymous() -> Self {
        Self {
            name: None,
            content: XsdSimpleTypeContent::Restriction(XsdSimpleRestriction::default()),
        }
    }
}

/// Simple type content model.
#[derive(Debug, Clone)]
pub enum XsdSimpleTypeContent {
    /// Restriction of another type
    Restriction(XsdSimpleRestriction),
    /// List of items
    List(XsdSimpleList),
    /// Union of types
    Union(XsdSimpleUnion),
}

/// Simple type restriction.
#[derive(Debug, Clone, Default)]
pub struct XsdSimpleRestriction {
    /// Base type being restricted
    pub base: Option<QName>,
    /// Inline base type
    pub inline_base: Option<Box<XsdSimpleType>>,
    /// Facets constraining the type
    pub facets: Vec<XsdFacet>,
}

/// Simple type list.
#[derive(Debug, Clone)]
pub struct XsdSimpleList {
    /// Item type reference
    pub item_type: Option<QName>,
    /// Inline item type
    pub inline_type: Option<Box<XsdSimpleType>>,
}

/// Simple type union.
#[derive(Debug, Clone)]
pub struct XsdSimpleUnion {
    /// Member type references
    pub member_types: Vec<QName>,
    /// Inline member types
    pub inline_types: Vec<XsdSimpleType>,
}

/// Facet constraint for simple types.
#[derive(Debug, Clone)]
pub enum XsdFacet {
    /// Enumeration value
    Enumeration(String),
    /// Pattern regex
    Pattern(String),
    /// Minimum length
    MinLength(u32),
    /// Maximum length
    MaxLength(u32),
    /// Exact length
    Length(u32),
    /// Minimum value (inclusive)
    MinInclusive(String),
    /// Maximum value (inclusive)
    MaxInclusive(String),
    /// Minimum value (exclusive)
    MinExclusive(String),
    /// Maximum value (exclusive)
    MaxExclusive(String),
    /// Total digits
    TotalDigits(u32),
    /// Fraction digits
    FractionDigits(u32),
    /// Whitespace handling
    WhiteSpace(WhiteSpaceValue),
}

/// Whitespace handling values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpaceValue {
    /// Preserve whitespace
    Preserve,
    /// Replace whitespace
    Replace,
    /// Collapse whitespace
    Collapse,
}

/// Complex type definition.
#[derive(Debug, Clone)]
pub struct XsdComplexType {
    /// Type name (None for anonymous types)
    pub name: Option<String>,
    /// Content model
    pub content: XsdComplexContent,
    /// Attribute declarations
    pub attributes: Vec<XsdAttribute>,
    /// Attribute group references
    pub attribute_groups: Vec<QName>,
    /// Whether this type is abstract
    pub is_abstract: bool,
    /// Whether content is mixed (text + elements)
    pub mixed: bool,
    /// Block constraint
    pub block: Option<DerivationControl>,
    /// Final constraint
    pub final_: Option<DerivationControl>,
}

impl XsdComplexType {
    /// Creates a new named complex type.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            content: XsdComplexContent::Empty,
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
            is_abstract: false,
            mixed: false,
            block: None,
            final_: None,
        }
    }

    /// Creates an anonymous complex type.
    pub fn anonymous() -> Self {
        Self {
            name: None,
            content: XsdComplexContent::Empty,
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
            is_abstract: false,
            mixed: false,
            block: None,
            final_: None,
        }
    }
}

/// Complex type content model.
#[derive(Debug, Clone)]
pub enum XsdComplexContent {
    /// Empty content
    Empty,
    /// Particle (sequence, choice, all, group ref)
    Particle(XsdParticle),
    /// Simple content (text with optional extension)
    SimpleContent(XsdSimpleContentDef),
    /// Complex content (extension or restriction)
    ComplexContent(XsdComplexContentDef),
}

/// Model group particle.
#[derive(Debug, Clone)]
pub enum XsdParticle {
    /// Sequence of elements (in order)
    Sequence(XsdSequence),
    /// Choice of elements (one of)
    Choice(XsdChoice),
    /// All (unordered set)
    All(XsdAll),
    /// Group reference
    GroupRef(QName),
    /// Any element wildcard
    Any(XsdAny),
}

/// Sequence compositor.
#[derive(Debug, Clone, Default)]
pub struct XsdSequence {
    /// Minimum occurrences
    pub min_occurs: Occurs,
    /// Maximum occurrences
    pub max_occurs: Occurs,
    /// Child particles
    pub particles: Vec<XsdParticleItem>,
}

/// Choice compositor.
#[derive(Debug, Clone, Default)]
pub struct XsdChoice {
    /// Minimum occurrences
    pub min_occurs: Occurs,
    /// Maximum occurrences
    pub max_occurs: Occurs,
    /// Child particles
    pub particles: Vec<XsdParticleItem>,
}

/// All compositor.
#[derive(Debug, Clone, Default)]
pub struct XsdAll {
    /// Minimum occurrences (0 or 1)
    pub min_occurs: Occurs,
    /// Maximum occurrences (always 1)
    pub max_occurs: Occurs,
    /// Elements (all children must be elements)
    pub elements: Vec<XsdElement>,
}

/// Item in a particle sequence/choice.
#[derive(Debug, Clone)]
pub enum XsdParticleItem {
    /// Element declaration
    Element(XsdElement),
    /// Nested sequence
    Sequence(XsdSequence),
    /// Nested choice
    Choice(XsdChoice),
    /// Group reference
    GroupRef(QName),
    /// Any wildcard
    Any(XsdAny),
}

/// Any element wildcard.
#[derive(Debug, Clone)]
pub struct XsdAny {
    /// Minimum occurrences
    pub min_occurs: Occurs,
    /// Maximum occurrences
    pub max_occurs: Occurs,
    /// Namespace constraint
    pub namespace: NamespaceConstraint,
    /// Process contents mode
    pub process_contents: ProcessContentsMode,
}

impl Default for XsdAny {
    fn default() -> Self {
        Self {
            min_occurs: Occurs::Count(1),
            max_occurs: Occurs::Count(1),
            namespace: NamespaceConstraint::Any,
            process_contents: ProcessContentsMode::Strict,
        }
    }
}

/// Namespace constraint for wildcards.
#[derive(Debug, Clone)]
pub enum NamespaceConstraint {
    /// Any namespace
    Any,
    /// Other namespaces (not target namespace)
    Other,
    /// Specific namespace URIs
    List(Vec<String>),
    /// Target namespace
    TargetNamespace,
    /// Local (no namespace)
    Local,
}

/// Process contents mode for wildcards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessContentsMode {
    /// Strict - must validate
    #[default]
    Strict,
    /// Lax - validate if schema available
    Lax,
    /// Skip - don't validate
    Skip,
}

/// Simple content definition.
#[derive(Debug, Clone)]
pub struct XsdSimpleContentDef {
    /// Extension or restriction
    pub derivation: XsdSimpleContentDerivation,
}

/// Simple content derivation.
#[derive(Debug, Clone)]
pub enum XsdSimpleContentDerivation {
    /// Extension of a simple type
    Extension(XsdSimpleContentExtension),
    /// Restriction of a simple type
    Restriction(XsdSimpleContentRestriction),
}

/// Simple content extension.
#[derive(Debug, Clone)]
pub struct XsdSimpleContentExtension {
    /// Base type being extended
    pub base: QName,
    /// Additional attributes
    pub attributes: Vec<XsdAttribute>,
    /// Additional attribute group references
    pub attribute_groups: Vec<QName>,
}

/// Simple content restriction.
#[derive(Debug, Clone)]
pub struct XsdSimpleContentRestriction {
    /// Base type being restricted
    pub base: QName,
    /// Facets constraining the type
    pub facets: Vec<XsdFacet>,
    /// Attributes
    pub attributes: Vec<XsdAttribute>,
    /// Attribute group references
    pub attribute_groups: Vec<QName>,
}

/// Complex content definition.
#[derive(Debug, Clone)]
pub struct XsdComplexContentDef {
    /// Whether content is mixed
    pub mixed: bool,
    /// Extension or restriction
    pub derivation: XsdComplexContentDerivation,
}

/// Complex content derivation.
#[derive(Debug, Clone)]
pub enum XsdComplexContentDerivation {
    /// Extension of a complex type
    Extension(XsdComplexContentExtension),
    /// Restriction of a complex type
    Restriction(XsdComplexContentRestriction),
}

/// Complex content extension.
#[derive(Debug, Clone)]
pub struct XsdComplexContentExtension {
    /// Base type being extended
    pub base: QName,
    /// Additional particle (sequence, choice, etc.)
    pub particle: Option<XsdParticle>,
    /// Additional attributes
    pub attributes: Vec<XsdAttribute>,
    /// Additional attribute group references
    pub attribute_groups: Vec<QName>,
}

/// Complex content restriction.
#[derive(Debug, Clone)]
pub struct XsdComplexContentRestriction {
    /// Base type being restricted
    pub base: QName,
    /// Particle (sequence, choice, etc.)
    pub particle: Option<XsdParticle>,
    /// Attributes
    pub attributes: Vec<XsdAttribute>,
    /// Attribute group references
    pub attribute_groups: Vec<QName>,
}

/// Attribute declaration.
#[derive(Debug, Clone)]
pub struct XsdAttribute {
    /// Attribute name
    pub name: Option<String>,
    /// Type reference
    pub type_ref: Option<QName>,
    /// Inline type definition
    pub inline_type: Option<XsdSimpleType>,
    /// Reference to another attribute
    pub ref_: Option<QName>,
    /// Use constraint
    pub use_: AttributeUse,
    /// Default value
    pub default: Option<String>,
    /// Fixed value
    pub fixed: Option<String>,
    /// Form (qualified/unqualified)
    pub form: Option<FormDefault>,
}

impl XsdAttribute {
    /// Creates a new attribute with a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            type_ref: None,
            inline_type: None,
            ref_: None,
            use_: AttributeUse::Optional,
            default: None,
            fixed: None,
            form: None,
        }
    }

    /// Creates an attribute reference.
    pub fn ref_(ref_name: QName) -> Self {
        Self {
            name: None,
            type_ref: None,
            inline_type: None,
            ref_: Some(ref_name),
            use_: AttributeUse::Optional,
            default: None,
            fixed: None,
            form: None,
        }
    }
}

/// Attribute use constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttributeUse {
    /// Required attribute
    Required,
    /// Optional attribute
    #[default]
    Optional,
    /// Prohibited attribute
    Prohibited,
}

/// Attribute group definition.
#[derive(Debug, Clone)]
pub struct XsdAttributeGroup {
    /// Group name
    pub name: Option<String>,
    /// Reference to another group
    pub ref_: Option<QName>,
    /// Attribute declarations
    pub attributes: Vec<XsdAttribute>,
    /// Nested attribute group references
    pub attribute_groups: Vec<QName>,
}

impl XsdAttributeGroup {
    /// Creates a new attribute group.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ref_: None,
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
        }
    }

    /// Creates an attribute group reference.
    pub fn ref_(ref_name: QName) -> Self {
        Self {
            name: None,
            ref_: Some(ref_name),
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
        }
    }
}

/// Model group definition.
#[derive(Debug, Clone)]
pub struct XsdGroup {
    /// Group name
    pub name: Option<String>,
    /// Reference to another group
    pub ref_: Option<QName>,
    /// Min occurs (for references)
    pub min_occurs: Occurs,
    /// Max occurs (for references)
    pub max_occurs: Occurs,
    /// Particle content
    pub particle: Option<XsdParticle>,
}

impl XsdGroup {
    /// Creates a new model group.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ref_: None,
            min_occurs: Occurs::Count(1),
            max_occurs: Occurs::Count(1),
            particle: None,
        }
    }

    /// Creates a model group reference.
    pub fn ref_(ref_name: QName) -> Self {
        Self {
            name: None,
            ref_: Some(ref_name),
            min_occurs: Occurs::Count(1),
            max_occurs: Occurs::Count(1),
            particle: None,
        }
    }
}

/// Derivation control (block/final).
#[derive(Debug, Clone)]
pub enum DerivationControl {
    /// All derivations blocked/final
    All,
    /// Specific derivations
    List(Vec<DerivationType>),
}

/// Derivation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivationType {
    /// Extension derivation
    Extension,
    /// Restriction derivation
    Restriction,
    /// Substitution
    Substitution,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =============================================================================
    // QName Tests
    // =============================================================================

    #[test]
    fn test_qname_new() {
        let qn = QName::new("localName");
        assert_eq!(qn.prefix, None);
        assert_eq!(qn.local, "localName");
    }

    #[test]
    fn test_qname_with_prefix() {
        let qn = QName::with_prefix("xs", "string");
        assert_eq!(qn.prefix, Some("xs".to_string()));
        assert_eq!(qn.local, "string");
    }

    #[test]
    fn test_qname_parse() {
        let qn = QName::parse("xs:string");
        assert_eq!(qn.prefix, Some("xs".to_string()));
        assert_eq!(qn.local, "string");

        let qn2 = QName::parse("localName");
        assert_eq!(qn2.prefix, None);
        assert_eq!(qn2.local, "localName");
    }

    #[test]
    fn test_qname_to_string_full() {
        let qn = QName::with_prefix("xs", "string");
        assert_eq!(qn.to_string_full(), "xs:string");

        let qn2 = QName::new("localName");
        assert_eq!(qn2.to_string_full(), "localName");
    }

    #[test]
    fn test_qname_display() {
        let qn = QName::with_prefix("xs", "string");
        assert_eq!(format!("{}", qn), "xs:string");

        let qn2 = QName::new("localName");
        assert_eq!(format!("{}", qn2), "localName");
    }

    // =============================================================================
    // Occurs Tests
    // =============================================================================

    #[test]
    fn test_occurs_parse() {
        assert_eq!(Occurs::parse("0"), Ok(Occurs::Count(0)));
        assert_eq!(Occurs::parse("1"), Ok(Occurs::Count(1)));
        assert_eq!(Occurs::parse("unbounded"), Ok(Occurs::Unbounded));

        // Test error cases
        assert!(Occurs::parse("-1").is_err());
        assert!(Occurs::parse("invalid").is_err());
    }

    #[test]
    fn test_occurs_default() {
        let occurs = Occurs::default();
        assert_eq!(occurs, Occurs::Count(1));
    }

    #[test]
    fn test_occurs_is_unbounded() {
        assert!(Occurs::Unbounded.is_unbounded());
        assert!(!Occurs::Count(1).is_unbounded());
        assert!(!Occurs::Count(0).is_unbounded());
    }

    #[test]
    fn test_occurs_to_option() {
        assert_eq!(Occurs::Count(5).to_option(), Some(5));
        assert_eq!(Occurs::Count(0).to_option(), Some(0));
        assert_eq!(Occurs::Unbounded.to_option(), None);
    }

    // =============================================================================
    // XsdSchema Tests
    // =============================================================================

    #[test]
    fn test_xsd_schema_default() {
        let schema = XsdSchema::new();
        assert!(schema.target_namespace.is_none());
        assert!(schema.elements.is_empty());
        assert!(schema.types.is_empty());
        assert!(schema.imports.is_empty());
        assert!(schema.includes.is_empty());
        assert!(schema.attributes.is_empty());
        assert_eq!(schema.element_form_default, FormDefault::Unqualified);
    }

    #[test]
    fn test_xsd_schema_with_target_namespace() {
        let schema = XsdSchema::new().with_target_namespace("http://example.com");
        assert_eq!(
            schema.target_namespace,
            Some("http://example.com".to_string())
        );
    }

    // =============================================================================
    // FormDefault Tests
    // =============================================================================

    #[test]
    fn test_form_default() {
        assert_eq!(FormDefault::default(), FormDefault::Unqualified);
        assert_ne!(FormDefault::Qualified, FormDefault::Unqualified);
    }

    // =============================================================================
    // XsdRedefine Tests
    // =============================================================================

    #[test]
    fn test_xsd_redefine_new() {
        let redefine = XsdRedefine::new("http://example.com/schema.xsd");
        assert_eq!(redefine.schema_location, "http://example.com/schema.xsd");
        assert!(redefine.simple_types.is_empty());
        assert!(redefine.complex_types.is_empty());
        assert!(redefine.groups.is_empty());
        assert!(redefine.attribute_groups.is_empty());
    }

    // =============================================================================
    // XsdIdentityConstraint Tests
    // =============================================================================

    #[test]
    fn test_xsd_identity_constraint_unique() {
        let constraint = XsdIdentityConstraint::unique("myUnique", "./item");
        assert_eq!(constraint.name, "myUnique");
        assert_eq!(constraint.constraint_type, XsdConstraintType::Unique);
        assert_eq!(constraint.selector, "./item");
        assert!(constraint.fields.is_empty());
        assert!(constraint.refer.is_none());
    }

    #[test]
    fn test_xsd_identity_constraint_key() {
        let constraint = XsdIdentityConstraint::key("myKey", "./item");
        assert_eq!(constraint.name, "myKey");
        assert_eq!(constraint.constraint_type, XsdConstraintType::Key);
        assert_eq!(constraint.selector, "./item");
    }

    #[test]
    fn test_xsd_identity_constraint_keyref() {
        let refer = QName::new("myKey");
        let constraint = XsdIdentityConstraint::keyref("myKeyRef", "./item", refer);
        assert_eq!(constraint.name, "myKeyRef");
        assert_eq!(constraint.constraint_type, XsdConstraintType::KeyRef);
        assert!(constraint.refer.is_some());
        assert_eq!(constraint.refer.unwrap().local, "myKey");
    }

    #[test]
    fn test_xsd_identity_constraint_with_field() {
        let constraint = XsdIdentityConstraint::unique("myUnique", "./item").with_field("@id");
        assert_eq!(constraint.fields, vec!["@id"]);
    }

    #[test]
    fn test_xsd_identity_constraint_with_fields() {
        let constraint =
            XsdIdentityConstraint::key("myKey", "./item").with_fields(["@id", "@name"]);
        assert_eq!(constraint.fields, vec!["@id", "@name"]);
    }

    // =============================================================================
    // XsdElement Tests
    // =============================================================================

    #[test]
    fn test_xsd_element_new() {
        let elem = XsdElement::new("myElement");
        assert_eq!(elem.name, "myElement");
        assert!(elem.type_ref.is_none());
        assert!(elem.inline_type.is_none());
        assert!(elem.ref_.is_none());
        assert_eq!(elem.min_occurs, Occurs::Count(1));
        assert_eq!(elem.max_occurs, Occurs::Count(1));
        assert!(!elem.is_abstract);
        assert!(!elem.nillable);
    }

    #[test]
    fn test_xsd_element_ref() {
        let elem = XsdElement::ref_(QName::new("otherElement"));
        assert_eq!(elem.name, "");
        assert!(elem.ref_.is_some());
        assert_eq!(elem.ref_.unwrap().local, "otherElement");
    }

    // =============================================================================
    // XsdTypeDef Tests
    // =============================================================================

    #[test]
    fn test_xsd_type_def_name_simple() {
        let simple = XsdSimpleType::new("myType");
        let type_def = XsdTypeDef::Simple(simple);
        assert_eq!(type_def.name(), Some("myType"));
    }

    #[test]
    fn test_xsd_type_def_name_complex() {
        let complex = XsdComplexType::new("myComplex");
        let type_def = XsdTypeDef::Complex(complex);
        assert_eq!(type_def.name(), Some("myComplex"));
    }

    #[test]
    fn test_xsd_type_def_name_anonymous() {
        let simple = XsdSimpleType::anonymous();
        let type_def = XsdTypeDef::Simple(simple);
        assert_eq!(type_def.name(), None);
    }

    #[test]
    fn test_xsd_type_def_is_simple() {
        let simple = XsdTypeDef::Simple(XsdSimpleType::new("t"));
        assert!(simple.is_simple());
        assert!(!simple.is_complex());
    }

    #[test]
    fn test_xsd_type_def_is_complex() {
        let complex = XsdTypeDef::Complex(XsdComplexType::new("t"));
        assert!(complex.is_complex());
        assert!(!complex.is_simple());
    }

    // =============================================================================
    // XsdSimpleType Tests
    // =============================================================================

    #[test]
    fn test_xsd_simple_type_new() {
        let simple = XsdSimpleType::new("mySimple");
        assert_eq!(simple.name, Some("mySimple".to_string()));
    }

    #[test]
    fn test_xsd_simple_type_anonymous() {
        let simple = XsdSimpleType::anonymous();
        assert_eq!(simple.name, None);
    }

    // =============================================================================
    // XsdComplexType Tests
    // =============================================================================

    #[test]
    fn test_xsd_complex_type_new() {
        let complex = XsdComplexType::new("myComplex");
        assert_eq!(complex.name, Some("myComplex".to_string()));
        assert!(!complex.is_abstract);
        assert!(!complex.mixed);
        assert!(complex.attributes.is_empty());
    }

    #[test]
    fn test_xsd_complex_type_anonymous() {
        let complex = XsdComplexType::anonymous();
        assert_eq!(complex.name, None);
    }

    // =============================================================================
    // XsdAny Tests
    // =============================================================================

    #[test]
    fn test_xsd_any_default() {
        let any = XsdAny::default();
        assert_eq!(any.min_occurs, Occurs::Count(1));
        assert_eq!(any.max_occurs, Occurs::Count(1));
        assert!(matches!(any.namespace, NamespaceConstraint::Any));
        assert_eq!(any.process_contents, ProcessContentsMode::Strict);
    }

    // =============================================================================
    // ProcessContentsMode Tests
    // =============================================================================

    #[test]
    fn test_process_contents_mode_default() {
        assert_eq!(ProcessContentsMode::default(), ProcessContentsMode::Strict);
    }

    // =============================================================================
    // XsdAttribute Tests
    // =============================================================================

    #[test]
    fn test_xsd_attribute_new() {
        let attr = XsdAttribute::new("myAttr");
        assert_eq!(attr.name, Some("myAttr".to_string()));
        assert!(attr.type_ref.is_none());
        assert!(attr.ref_.is_none());
        assert_eq!(attr.use_, AttributeUse::Optional);
    }

    #[test]
    fn test_xsd_attribute_ref() {
        let attr = XsdAttribute::ref_(QName::new("otherAttr"));
        assert_eq!(attr.name, None);
        assert!(attr.ref_.is_some());
        assert_eq!(attr.ref_.unwrap().local, "otherAttr");
    }

    // =============================================================================
    // AttributeUse Tests
    // =============================================================================

    #[test]
    fn test_attribute_use_default() {
        assert_eq!(AttributeUse::default(), AttributeUse::Optional);
    }

    // =============================================================================
    // XsdAttributeGroup Tests
    // =============================================================================

    #[test]
    fn test_xsd_attribute_group_new() {
        let group = XsdAttributeGroup::new("myGroup");
        assert_eq!(group.name, Some("myGroup".to_string()));
        assert!(group.ref_.is_none());
        assert!(group.attributes.is_empty());
    }

    #[test]
    fn test_xsd_attribute_group_ref() {
        let group = XsdAttributeGroup::ref_(QName::new("otherGroup"));
        assert_eq!(group.name, None);
        assert!(group.ref_.is_some());
        assert_eq!(group.ref_.unwrap().local, "otherGroup");
    }

    // =============================================================================
    // XsdGroup Tests
    // =============================================================================

    #[test]
    fn test_xsd_group_new() {
        let group = XsdGroup::new("myGroup");
        assert_eq!(group.name, Some("myGroup".to_string()));
        assert!(group.ref_.is_none());
        assert!(group.particle.is_none());
        assert_eq!(group.min_occurs, Occurs::Count(1));
        assert_eq!(group.max_occurs, Occurs::Count(1));
    }

    #[test]
    fn test_xsd_group_ref() {
        let group = XsdGroup::ref_(QName::new("otherGroup"));
        assert_eq!(group.name, None);
        assert!(group.ref_.is_some());
        assert_eq!(group.ref_.unwrap().local, "otherGroup");
    }

    // =============================================================================
    // XsdSequence/XsdChoice/XsdAll Tests
    // =============================================================================

    #[test]
    fn test_xsd_sequence_default() {
        let seq = XsdSequence::default();
        assert!(seq.particles.is_empty());
    }

    #[test]
    fn test_xsd_choice_default() {
        let choice = XsdChoice::default();
        assert!(choice.particles.is_empty());
    }

    #[test]
    fn test_xsd_all_default() {
        let all = XsdAll::default();
        assert!(all.elements.is_empty());
    }

    // =============================================================================
    // XsdSimpleRestriction Tests
    // =============================================================================

    #[test]
    fn test_xsd_simple_restriction_default() {
        let restriction = XsdSimpleRestriction::default();
        assert!(restriction.base.is_none());
        assert!(restriction.inline_base.is_none());
        assert!(restriction.facets.is_empty());
    }

    // =============================================================================
    // NamespaceConstraint Tests
    // =============================================================================

    #[test]
    fn test_namespace_constraint_variants() {
        let any = NamespaceConstraint::Any;
        let other = NamespaceConstraint::Other;
        let list = NamespaceConstraint::List(vec!["http://example.com".to_string()]);
        let target = NamespaceConstraint::TargetNamespace;
        let local = NamespaceConstraint::Local;

        // Just test that all variants exist and can be created
        assert!(matches!(any, NamespaceConstraint::Any));
        assert!(matches!(other, NamespaceConstraint::Other));
        assert!(matches!(list, NamespaceConstraint::List(_)));
        assert!(matches!(target, NamespaceConstraint::TargetNamespace));
        assert!(matches!(local, NamespaceConstraint::Local));
    }

    // =============================================================================
    // XsdConstraintType Tests
    // =============================================================================

    #[test]
    fn test_xsd_constraint_type_variants() {
        assert_eq!(XsdConstraintType::Unique, XsdConstraintType::Unique);
        assert_eq!(XsdConstraintType::Key, XsdConstraintType::Key);
        assert_eq!(XsdConstraintType::KeyRef, XsdConstraintType::KeyRef);
        assert_ne!(XsdConstraintType::Unique, XsdConstraintType::Key);
    }

    // =============================================================================
    // DerivationControl/DerivationType Tests
    // =============================================================================

    #[test]
    fn test_derivation_control_all() {
        let control = DerivationControl::All;
        assert!(matches!(control, DerivationControl::All));
    }

    #[test]
    fn test_derivation_control_list() {
        let control =
            DerivationControl::List(vec![DerivationType::Extension, DerivationType::Restriction]);
        if let DerivationControl::List(types) = control {
            assert_eq!(types.len(), 2);
            assert!(types.contains(&DerivationType::Extension));
            assert!(types.contains(&DerivationType::Restriction));
        } else {
            panic!("Expected List variant");
        }
    }

    #[test]
    fn test_derivation_type_variants() {
        assert_eq!(DerivationType::Extension, DerivationType::Extension);
        assert_eq!(DerivationType::Restriction, DerivationType::Restriction);
        assert_eq!(DerivationType::Substitution, DerivationType::Substitution);
        assert_ne!(DerivationType::Extension, DerivationType::Restriction);
    }

    // =============================================================================
    // WhiteSpaceValue Tests
    // =============================================================================

    #[test]
    fn test_whitespace_value_variants() {
        assert_eq!(WhiteSpaceValue::Preserve, WhiteSpaceValue::Preserve);
        assert_eq!(WhiteSpaceValue::Replace, WhiteSpaceValue::Replace);
        assert_eq!(WhiteSpaceValue::Collapse, WhiteSpaceValue::Collapse);
        assert_ne!(WhiteSpaceValue::Preserve, WhiteSpaceValue::Collapse);
    }

    // =============================================================================
    // XsdFacet Tests
    // =============================================================================

    #[test]
    fn test_xsd_facet_variants() {
        let facets = vec![
            XsdFacet::Enumeration("value".to_string()),
            XsdFacet::Pattern("[0-9]+".to_string()),
            XsdFacet::MinLength(1),
            XsdFacet::MaxLength(100),
            XsdFacet::Length(10),
            XsdFacet::MinInclusive("0".to_string()),
            XsdFacet::MaxInclusive("100".to_string()),
            XsdFacet::MinExclusive("0".to_string()),
            XsdFacet::MaxExclusive("100".to_string()),
            XsdFacet::TotalDigits(10),
            XsdFacet::FractionDigits(2),
            XsdFacet::WhiteSpace(WhiteSpaceValue::Collapse),
        ];
        assert_eq!(facets.len(), 12);
    }

    // =============================================================================
    // XsdSimpleTypeContent Tests
    // =============================================================================

    #[test]
    fn test_xsd_simple_type_content_restriction() {
        let content = XsdSimpleTypeContent::Restriction(XsdSimpleRestriction::default());
        assert!(matches!(content, XsdSimpleTypeContent::Restriction(_)));
    }

    #[test]
    fn test_xsd_simple_type_content_list() {
        let content = XsdSimpleTypeContent::List(XsdSimpleList {
            item_type: Some(QName::new("string")),
            inline_type: None,
        });
        assert!(matches!(content, XsdSimpleTypeContent::List(_)));
    }

    #[test]
    fn test_xsd_simple_type_content_union() {
        let content = XsdSimpleTypeContent::Union(XsdSimpleUnion {
            member_types: vec![QName::new("string"), QName::new("int")],
            inline_types: Vec::new(),
        });
        assert!(matches!(content, XsdSimpleTypeContent::Union(_)));
    }

    // =============================================================================
    // XsdComplexContent Tests
    // =============================================================================

    #[test]
    fn test_xsd_complex_content_empty() {
        let content = XsdComplexContent::Empty;
        assert!(matches!(content, XsdComplexContent::Empty));
    }

    #[test]
    fn test_xsd_complex_content_particle() {
        let content = XsdComplexContent::Particle(XsdParticle::Sequence(XsdSequence::default()));
        assert!(matches!(content, XsdComplexContent::Particle(_)));
    }

    // =============================================================================
    // XsdParticle Tests
    // =============================================================================

    #[test]
    fn test_xsd_particle_variants() {
        let seq = XsdParticle::Sequence(XsdSequence::default());
        let choice = XsdParticle::Choice(XsdChoice::default());
        let all = XsdParticle::All(XsdAll::default());
        let group_ref = XsdParticle::GroupRef(QName::new("myGroup"));
        let any = XsdParticle::Any(XsdAny::default());

        assert!(matches!(seq, XsdParticle::Sequence(_)));
        assert!(matches!(choice, XsdParticle::Choice(_)));
        assert!(matches!(all, XsdParticle::All(_)));
        assert!(matches!(group_ref, XsdParticle::GroupRef(_)));
        assert!(matches!(any, XsdParticle::Any(_)));
    }

    // =============================================================================
    // XsdParticleItem Tests
    // =============================================================================

    #[test]
    fn test_xsd_particle_item_variants() {
        let elem = XsdParticleItem::Element(XsdElement::new("e"));
        let seq = XsdParticleItem::Sequence(XsdSequence::default());
        let choice = XsdParticleItem::Choice(XsdChoice::default());
        let group_ref = XsdParticleItem::GroupRef(QName::new("g"));
        let any = XsdParticleItem::Any(XsdAny::default());

        assert!(matches!(elem, XsdParticleItem::Element(_)));
        assert!(matches!(seq, XsdParticleItem::Sequence(_)));
        assert!(matches!(choice, XsdParticleItem::Choice(_)));
        assert!(matches!(group_ref, XsdParticleItem::GroupRef(_)));
        assert!(matches!(any, XsdParticleItem::Any(_)));
    }
}
