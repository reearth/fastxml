//! XSD model group definitions.

use super::occurs::Occurs;
use super::particles::XsdParticle;
use super::qname::QName;

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
