//! XSD particle types (sequence, choice, all, any).

use super::elements::XsdElement;
use super::occurs::Occurs;
use super::qname::QName;

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
