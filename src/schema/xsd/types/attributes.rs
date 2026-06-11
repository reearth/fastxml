//! XSD attribute group definitions.

use super::elements::XsdAttribute;
use super::qname::QName;

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
    /// Attribute wildcard (xs:anyAttribute)
    pub any_attribute: Option<super::particles::XsdAny>,
}

impl XsdAttributeGroup {
    /// Creates a new attribute group.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ref_: None,
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
            any_attribute: None,
        }
    }

    /// Creates an attribute group reference.
    pub fn ref_(ref_name: QName) -> Self {
        Self {
            name: None,
            ref_: Some(ref_name),
            attributes: Vec::new(),
            attribute_groups: Vec::new(),
            any_attribute: None,
        }
    }
}
