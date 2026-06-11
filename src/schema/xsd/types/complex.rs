//! XSD complex type definitions.

use super::elements::XsdAttribute;
use super::particles::{XsdAny, XsdParticle};
use super::qname::QName;
use super::simple::XsdFacet;

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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<XsdAny>,
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
            any_attribute: None,
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
            any_attribute: None,
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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<XsdAny>,
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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<XsdAny>,
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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<XsdAny>,
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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<XsdAny>,
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
