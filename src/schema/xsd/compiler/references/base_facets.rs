//! Facet checks that follow a restriction's base: what a list or union base
//! allows, and the range a restriction inherits.

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::xsd::compiler::facet_checks::{FacetBase, InheritedFacets, check_against_base};
use crate::schema::xsd::primitive::PrimitiveKind;
use crate::schema::xsd::types::*;

use super::{Checker, tns_key};

impl Checker<'_> {
    /// Checks a restriction step's facets against its base's variety and
    /// the range it inherits. Bases are followed through simple types of
    /// this schema document down to a built-in; any other base is skipped.
    pub(super) fn check_facets_against_base(
        &self,
        facets: &[XsdFacet],
        base: &QName,
    ) -> Result<()> {
        if let Some(st) = self.own_simple_type(base) {
            let allowed: fn(&XsdFacet) -> bool = match &st.content {
                XsdSimpleTypeContent::Restriction(_) => |_| true,
                XsdSimpleTypeContent::List(_) => |f| {
                    matches!(
                        f,
                        XsdFacet::Length(_)
                            | XsdFacet::MinLength(_)
                            | XsdFacet::MaxLength(_)
                            | XsdFacet::Pattern(_)
                            | XsdFacet::Enumeration(_)
                            | XsdFacet::WhiteSpace(_)
                    )
                },
                XsdSimpleTypeContent::Union(_) => {
                    |f| matches!(f, XsdFacet::Pattern(_) | XsdFacet::Enumeration(_))
                }
            };
            if let Some(facet) = facets.iter().find(|f| !allowed(f)) {
                return Err(SchemaError::InvalidSchema {
                    message: format!(
                        "facet {facet:?} is not applicable to the list or union type '{}'",
                        base.local.trim()
                    ),
                }
                .into());
            }
        }
        if let Some((inherited, kind)) = self.facets_of(base, 0) {
            check_against_base(facets, &inherited, kind)?;
        }
        Ok(())
    }

    /// Attribute Declaration Properties Correct (§3.2.6 clause 2): an
    /// attribute's `default`/`fixed` value must be valid for its type.
    pub(super) fn check_attribute_value(&self, attr: &XsdAttribute) -> Result<()> {
        let value = attr.default.as_deref().or(attr.fixed.as_deref());
        let name = attr.name.as_deref().unwrap_or_default();
        let inline = attr.inline_type.as_ref();
        self.check_value_constraint("attribute", name, value, attr.type_ref.as_ref(), inline)
    }

    /// Element Declaration Properties Correct (§3.3.6 clause 2): an
    /// element's `default`/`fixed` value must be valid for its simple type.
    pub(super) fn check_element_value(&self, element: &XsdElement) -> Result<()> {
        let value = element.default.as_deref().or(element.fixed.as_deref());
        if value.is_none() {
            return Ok(());
        }
        let invalid =
            |message: String| -> Result<()> { Err(SchemaError::InvalidSchema { message }.into()) };
        let complex = match (element.inline_type.as_deref(), &element.type_ref) {
            (Some(XsdTypeDef::Complex(ct)), _) => Some(ct),
            (None, Some(t)) => self.own_complex_type(t),
            _ => None,
        };
        // Clause 2: a complex type takes a value constraint only with
        // simple or mixed content.
        if let Some(ct) = complex {
            if !ct.mixed
                && matches!(
                    ct.content,
                    XsdComplexContent::Particle(_) | XsdComplexContent::Empty
                )
            {
                return invalid(format!(
                    "element '{}': default/fixed needs simple or mixed content",
                    element.name
                ));
            }
            return Ok(());
        }
        let inline = match element.inline_type.as_deref() {
            Some(XsdTypeDef::Simple(st)) => Some(st),
            _ => None,
        };
        let type_ref = element.type_ref.as_ref();
        // Clause 5: no value constraint on an xs:ID-derived type.
        let kind = match (type_ref, inline) {
            (Some(t), _) => self.facets_of(t, 0).and_then(|(_, kind)| kind),
            (None, Some(inline)) => self.facets_of_type(inline, 0).and_then(|(_, kind)| kind),
            (None, None) => None,
        };
        if kind == Some(PrimitiveKind::Id) {
            return invalid(format!(
                "element '{}': an xs:ID-typed element cannot have default/fixed",
                element.name
            ));
        }
        self.check_value_constraint("element", &element.name, value, type_ref, inline)
    }

    /// The top-level complex type `qname` names in this schema document.
    pub(super) fn own_complex_type(&self, qname: &QName) -> Option<&XsdComplexType> {
        if self.resolve_ns(qname)? != tns_key(self.schema) {
            return None;
        }
        let local = qname.local.trim();
        self.schema.types.iter().find_map(|t| match t {
            XsdTypeDef::Complex(ct) if ct.name.as_deref() == Some(local) => Some(ct),
            _ => None,
        })
    }

    /// Checks a value constraint in the primitive value space of the simple
    /// type `type_ref` names or `inline` defines. Other types are skipped.
    fn check_value_constraint(
        &self,
        what: &str,
        name: &str,
        value: Option<&str>,
        type_ref: Option<&QName>,
        inline: Option<&XsdSimpleType>,
    ) -> Result<()> {
        let Some(value) = value else {
            return Ok(());
        };
        let kind = match (type_ref, inline) {
            (Some(t), _) => self.facets_of(t, 0).and_then(|(_, kind)| kind),
            (None, Some(inline)) => self.facets_of_type(inline, 0).and_then(|(_, kind)| kind),
            (None, None) => None,
        };
        if let Some(kind) = kind
            && kind.validate(value).is_err()
        {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "{what} '{name}': default/fixed value '{value}' is not valid for its type"
                ),
            }
            .into());
        }
        Ok(())
    }

    /// Checks a restriction step's facets against its anonymous base.
    pub(super) fn check_facets_against_inline_base(
        &self,
        facets: &[XsdFacet],
        base: &XsdSimpleType,
    ) -> Result<()> {
        if let Some((inherited, kind)) = self.facets_of_type(base, 0) {
            check_against_base(facets, &inherited, kind)?;
        }
        Ok(())
    }

    /// The facets the type `qname` names has, with its primitive kind.
    /// `None` when the type is neither built in nor defined in this schema
    /// document.
    fn facets_of(
        &self,
        qname: &QName,
        depth: usize,
    ) -> Option<(InheritedFacets, Option<PrimitiveKind>)> {
        match self.own_simple_type(qname) {
            Some(st) => self.facets_of_type(st, depth),
            None => match self.facet_base(Some(qname)) {
                FacetBase::Unknown => None,
                FacetBase::Primitive(kind) => Some((
                    InheritedFacets::builtin(FacetBase::Primitive(kind)),
                    Some(kind),
                )),
                base => Some((InheritedFacets::builtin(base), None)),
            },
        }
    }

    /// The facets the simple type `st` has, with its primitive kind.
    fn facets_of_type(
        &self,
        st: &XsdSimpleType,
        depth: usize,
    ) -> Option<(InheritedFacets, Option<PrimitiveKind>)> {
        if depth > 64 {
            return None; // a derivation cycle, reported elsewhere
        }
        match &st.content {
            XsdSimpleTypeContent::Restriction(r) => {
                let (inherited, kind) = match (&r.base, &r.inline_base) {
                    (Some(base), _) => self.facets_of(base, depth + 1)?,
                    (None, Some(inline)) => self.facets_of_type(inline, depth + 1)?,
                    (None, None) => return None,
                };
                Some((inherited.restricted_by(&r.facets), kind))
            }
            XsdSimpleTypeContent::List(_) => Some((InheritedFacets::list(None), None)),
            XsdSimpleTypeContent::Union(_) => None,
        }
    }
    /// The top-level simple type `qname` names in this schema document.
    fn own_simple_type(&self, qname: &QName) -> Option<&XsdSimpleType> {
        if self.resolve_ns(qname)? != tns_key(self.schema) {
            return None;
        }
        let local = qname.local.trim();
        self.schema.types.iter().find_map(|t| match t {
            XsdTypeDef::Simple(st) if st.name.as_deref() == Some(local) => Some(st),
            _ => None,
        })
    }
}
