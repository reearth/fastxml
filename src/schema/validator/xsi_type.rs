//! `xsi:type` substitution resolution and derivation checking.

use crate::schema::types::{
    CompiledSchema, ComplexType, DerivationMethod, ElementDef, NsName, TypeDef,
};

/// A resolved `xsi:type`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct XsiType {
    /// The type's schema key, for the string-keyed lookups.
    pub key: String,
    /// The type's expanded name, when the QName's namespace is known. It is
    /// the authoritative identity: the bare `key` can name a same-local-name
    /// type of another namespace.
    pub ns: Option<NsName>,
}

/// The type an element declaration gives its instances, as far as `xsi:type`
/// checking is concerned.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DeclaredType<'a> {
    /// A named type (`type="..."`), by its schema key.
    Named(&'a str),
    /// An anonymous type defined inline in the declaration. No named type
    /// can be derived from it, so any `xsi:type` is rejected.
    Anonymous,
    /// No type at all: `xs:anyType`, from which every type derives.
    AnyType,
}

impl<'a> DeclaredType<'a> {
    /// The declared type of an element declaration.
    pub(crate) fn of(elem: &'a ElementDef) -> Self {
        Self::from_parts(elem.type_ref.as_deref(), elem.inline_type.is_some())
    }

    /// The declared type from a named-type key and whether an anonymous type
    /// is present (a named type wins, as in the declaration itself).
    pub(crate) fn from_parts(type_ref: Option<&'a str>, has_inline: bool) -> Self {
        match type_ref {
            Some(name) => Self::Named(name),
            None if has_inline => Self::Anonymous,
            None => Self::AnyType,
        }
    }
}

/// Resolves an `xsi:type` attribute value against the schema and checks the
/// substitution is allowed for the declared type.
///
/// `resolve_prefix` maps the QName's prefix (`""` for none) to the namespace
/// URI in scope at the carrying element — the instance document's own
/// declarations, which are the authoritative interpretation of the QName.
/// An unprefixed name takes the in-scope default namespace (no namespace when
/// there is none). Whenever the QName's namespace is known, the type must
/// exist under that expanded name, and that definition anchors the
/// derivation-chain walk; the legacy string heuristic (qualified key, then
/// bare local) is used only for a prefix that is not bound at all.
///
/// Returns the schema key of the substituted type on success, or an error
/// message when the type is unknown, not derived from the declared type, or
/// the derivation is blocked.
pub(crate) fn resolve_xsi_type(
    schema: &CompiledSchema,
    declared: DeclaredType<'_>,
    xsi_type: &str,
    resolve_prefix: impl Fn(&str) -> Option<String>,
) -> Result<XsiType, String> {
    let xsi_type = xsi_type.trim();
    let (prefix, local) = match xsi_type.split_once(':') {
        Some((p, l)) => (p, l),
        None => ("", xsi_type),
    };

    // The QName's namespace: `None` only for an unbound prefix.
    let namespace: Option<String> = if prefix.is_empty() {
        Some(resolve_prefix("").unwrap_or_default())
    } else {
        resolve_prefix(prefix)
    };
    let ns_def: Option<&TypeDef> = namespace
        .as_deref()
        .and_then(|uri| schema.type_ns(uri, local));
    if namespace.is_some() && ns_def.is_none() {
        return Err(format!("xsi:type '{}' is not defined in schema", xsi_type));
    }

    // Find the string key under either its qualified or local name (this is
    // what downstream lookups consume; the value stays as written for
    // message stability).
    let resolved = |key: String| XsiType {
        key,
        ns: namespace
            .as_deref()
            .filter(|_| ns_def.is_some())
            .map(|uri| NsName::new(uri, local)),
    };
    let key = if schema.get_type(xsi_type).is_some() {
        xsi_type.to_string()
    } else if schema.get_type(local).is_some() {
        local.to_string()
    } else if ns_def.is_some() {
        // Resolvable only through the ns map; use the local name as the
        // downstream key (bare local keys are registered for every type).
        local.to_string()
    } else {
        return Err(format!("xsi:type '{}' is not defined in schema", xsi_type));
    };

    let declared = match declared {
        DeclaredType::Named(name) => name,
        DeclaredType::AnyType => return Ok(resolved(key)),
        DeclaredType::Anonymous => {
            return Err(format!(
                "xsi:type '{}' is not derived from the element's anonymous type",
                xsi_type
            ));
        }
    };
    let declared_local = declared.rsplit(':').next().unwrap_or(declared);

    // Every type derives from xs:anyType (and every simple type from
    // xs:anySimpleType), so substitution is always allowed.
    if declared_local == "anyType" || declared_local == "anySimpleType" {
        return Ok(resolved(key));
    }

    if local_name(&key) == declared_local {
        return Ok(resolved(key)); // same type
    }

    // Walk the substituted type's derivation chain up to the declared type,
    // collecting the derivation methods used along the way. The walk hops
    // definitions ns-first (compile-time resolved base_ns, string fallback),
    // anchored at the instance-namespace-resolved definition when available.
    let mut methods: Vec<DerivationMethod> = Vec::new();
    let mut current: Option<&TypeDef> = ns_def.or_else(|| schema.get_type(&key));
    for _ in 0..32 {
        let (base, base_def, method) = match current {
            Some(TypeDef::Complex(c)) => (
                c.base_type.as_deref(),
                c.base_type
                    .as_deref()
                    .and_then(|b| schema.type_by_ref(c.base_ns.as_ref(), b)),
                c.derivation.unwrap_or(DerivationMethod::Restriction),
            ),
            Some(TypeDef::Simple(s)) => (
                s.base_type.as_deref(),
                s.base_type
                    .as_deref()
                    .and_then(|b| schema.type_by_ref(s.base_ns.as_ref(), b)),
                DerivationMethod::Restriction,
            ),
            None => (None, None, DerivationMethod::Restriction),
        };
        let Some(base) = base else {
            return Err(format!(
                "xsi:type '{}' is not derived from declared type '{}'",
                xsi_type, declared
            ));
        };
        methods.push(method);

        if local_name(base) == declared_local {
            // Reached the declared type: check its block constraints.
            if let Some(TypeDef::Complex(declared_type)) = schema
                .get_type(declared)
                .or_else(|| schema.get_type(declared_local))
            {
                if let Some(blocked) = blocked_method(declared_type, &methods) {
                    return Err(format!(
                        "xsi:type '{}' uses {} derivation, which is blocked by type '{}'",
                        xsi_type,
                        match blocked {
                            DerivationMethod::Extension => "extension",
                            DerivationMethod::Restriction => "restriction",
                        },
                        declared
                    ));
                }
            }
            return Ok(resolved(key));
        }
        current = base_def;
    }

    Err(format!(
        "xsi:type '{}' is not derived from declared type '{}'",
        xsi_type, declared
    ))
}

/// Returns the first derivation method in `methods` blocked by the declared
/// type's `block` attribute, if any.
fn blocked_method(
    declared: &ComplexType,
    methods: &[DerivationMethod],
) -> Option<DerivationMethod> {
    methods.iter().copied().find(|m| declared.block.blocks(*m))
}

fn local_name(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> CompiledSchema {
        let xsd = br#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:complexType name="B" block="extension">
    <xs:sequence><xs:element name="foo" type="xs:string" minOccurs="0"/></xs:sequence>
  </xs:complexType>
  <xs:complexType name="De">
    <xs:complexContent><xs:extension base="B"/></xs:complexContent>
  </xs:complexType>
  <xs:complexType name="Dr">
    <xs:complexContent>
      <xs:restriction base="B">
        <xs:sequence><xs:element name="foo" type="xs:string" minOccurs="0"/></xs:sequence>
      </xs:restriction>
    </xs:complexContent>
  </xs:complexType>
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence maxOccurs="unbounded"><xs:element name="item" type="B"/></xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
        crate::schema::xsd::parse_xsd(xsd).expect("parse schema")
    }

    #[test]
    fn extension_blocked_by_declared_type() {
        let s = schema();
        let result = resolve_xsi_type(&s, DeclaredType::Named("B"), "De", |_| None);
        assert!(result.is_err(), "extension is blocked, got {:?}", result);
    }

    #[test]
    fn restriction_allowed() {
        let s = schema();
        let result = resolve_xsi_type(&s, DeclaredType::Named("B"), "Dr", |_| None);
        assert_eq!(result.map(|t| t.key).as_deref(), Ok("Dr"));
    }

    #[test]
    fn unknown_type_rejected() {
        let s = schema();
        assert!(resolve_xsi_type(&s, DeclaredType::Named("B"), "NoSuchType", |_| None).is_err());
    }

    #[test]
    fn same_type_allowed() {
        let s = schema();
        assert_eq!(
            resolve_xsi_type(&s, DeclaredType::Named("B"), "B", |_| None)
                .map(|t| t.key)
                .as_deref(),
            Ok("B")
        );
    }

    #[test]
    fn anonymous_declared_type_rejects_any_substitution() {
        let s = schema();
        assert!(resolve_xsi_type(&s, DeclaredType::Anonymous, "B", |_| None).is_err());
        assert_eq!(
            resolve_xsi_type(&s, DeclaredType::AnyType, "B", |_| None)
                .map(|t| t.key)
                .as_deref(),
            Ok("B")
        );
    }
}
