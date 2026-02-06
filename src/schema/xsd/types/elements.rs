//! XSD element and attribute declarations.

use super::constraints::XsdIdentityConstraint;
use super::occurs::Occurs;
use super::qname::QName;
use super::schema::FormDefault;
use super::simple::XsdSimpleType;

/// Type definition (simple or complex).
#[derive(Debug, Clone)]
pub enum XsdTypeDef {
    /// Simple type
    Simple(super::simple::XsdSimpleType),
    /// Complex type
    Complex(super::complex::XsdComplexType),
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
