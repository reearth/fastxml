//! Stack frame types for XSD parsing state.

use crate::schema::xsd::types::*;

/// Parser state stack frame.
#[derive(Debug)]
pub(crate) enum StackFrame {
    /// Parsing xs:schema
    Schema,
    /// Parsing xs:element
    Element(XsdElement),
    /// Parsing xs:complexType
    ComplexType(XsdComplexType),
    /// Parsing xs:simpleType
    SimpleType(XsdSimpleType),
    /// Parsing xs:sequence
    Sequence(XsdSequence),
    /// Parsing xs:choice
    Choice(XsdChoice),
    /// Parsing xs:all
    All(XsdAll),
    /// Parsing xs:attribute
    Attribute(XsdAttribute),
    /// Parsing xs:attributeGroup
    AttributeGroup(XsdAttributeGroup),
    /// Parsing xs:group
    Group(XsdGroup),
    /// Parsing xs:restriction (simple type)
    SimpleRestriction(XsdSimpleRestriction),
    /// Parsing xs:extension (simple content)
    SimpleContentExtension(XsdSimpleContentExtension),
    /// Parsing xs:restriction (simple content)
    SimpleContentRestriction(XsdSimpleContentRestriction),
    /// Parsing xs:complexContent
    ComplexContent { mixed: bool },
    /// Parsing xs:simpleContent
    SimpleContent,
    /// Parsing xs:extension (complex content)
    ComplexContentExtension(XsdComplexContentExtension),
    /// Parsing xs:restriction (complex content)
    ComplexContentRestriction(XsdComplexContentRestriction),
    /// Parsing xs:list
    SimpleList(XsdSimpleList),
    /// Parsing xs:union
    SimpleUnion(XsdSimpleUnion),
    /// Parsing xs:any
    Any(XsdAny),
    /// Parsing xs:anyAttribute (ignored for now)
    AnyAttribute(XsdAny),
    /// Parsing annotation (skipped)
    Annotation,
    /// Parsing documentation (skipped)
    Documentation,
    /// Parsing appinfo (skipped)
    AppInfo,
    /// Parsing xs:unique identity constraint
    Unique(XsdIdentityConstraint),
    /// Parsing xs:key identity constraint
    Key(XsdIdentityConstraint),
    /// Parsing xs:keyref identity constraint
    KeyRef(XsdIdentityConstraint),
    /// Parsing xs:redefine
    Redefine(XsdRedefine),
}
