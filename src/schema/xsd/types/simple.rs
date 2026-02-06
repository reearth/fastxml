//! XSD simple type definitions.

use super::qname::QName;

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
