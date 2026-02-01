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

    /// Converts to an Option<u32> where None means unbounded.
    pub fn to_option(&self) -> Option<u32> {
        match self {
            Occurs::Count(n) => Some(*n),
            Occurs::Unbounded => None,
        }
    }

    /// Parses from a string, handling "unbounded".
    pub fn parse(s: &str) -> Self {
        if s == "unbounded" {
            Occurs::Unbounded
        } else {
            s.parse::<u32>().map(Occurs::Count).unwrap_or(Occurs::Count(1))
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
    pub fn keyref(
        name: impl Into<String>,
        selector: impl Into<String>,
        refer: QName,
    ) -> Self {
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
    fn test_occurs_parse() {
        assert_eq!(Occurs::parse("0"), Occurs::Count(0));
        assert_eq!(Occurs::parse("1"), Occurs::Count(1));
        assert_eq!(Occurs::parse("unbounded"), Occurs::Unbounded);
    }

    #[test]
    fn test_xsd_schema_default() {
        let schema = XsdSchema::new();
        assert!(schema.target_namespace.is_none());
        assert!(schema.elements.is_empty());
        assert!(schema.types.is_empty());
    }
}
