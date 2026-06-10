//! `xsi:type` substitution resolution and derivation checking.

use crate::schema::types::{CompiledSchema, ComplexType, DerivationMethod, TypeDef};

/// Resolves an `xsi:type` attribute value against the schema and checks the
/// substitution is allowed for the declared type.
///
/// Returns the schema key of the substituted type on success, or an error
/// message when the type is unknown, not derived from the declared type, or
/// the derivation is blocked.
pub(crate) fn resolve_xsi_type(
    schema: &CompiledSchema,
    declared: Option<&str>,
    xsi_type: &str,
) -> Result<String, String> {
    let xsi_type = xsi_type.trim();
    let local = xsi_type.rsplit(':').next().unwrap_or(xsi_type);

    // Find the type under either its qualified or local name.
    let key = if schema.get_type(xsi_type).is_some() {
        xsi_type.to_string()
    } else if schema.get_type(local).is_some() {
        local.to_string()
    } else {
        return Err(format!("xsi:type '{}' is not defined in schema", xsi_type));
    };

    let Some(declared) = declared else {
        return Ok(key); // no declared type to conflict with
    };
    let declared_local = declared.rsplit(':').next().unwrap_or(declared);

    if local_name(&key) == declared_local {
        return Ok(key); // same type
    }

    // Walk the substituted type's derivation chain up to the declared type,
    // collecting the derivation methods used along the way.
    let mut methods: Vec<DerivationMethod> = Vec::new();
    let mut current = key.clone();
    for _ in 0..32 {
        let (base, method) = match schema.get_type(&current) {
            Some(TypeDef::Complex(c)) => (
                c.base_type.clone(),
                c.derivation.unwrap_or(DerivationMethod::Restriction),
            ),
            Some(TypeDef::Simple(s)) => (s.base_type.clone(), DerivationMethod::Restriction),
            None => (None, DerivationMethod::Restriction),
        };
        let Some(base) = base else {
            return Err(format!(
                "xsi:type '{}' is not derived from declared type '{}'",
                xsi_type, declared
            ));
        };
        methods.push(method);

        if local_name(&base) == declared_local {
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
            return Ok(key);
        }
        current = base;
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
        let result = resolve_xsi_type(&s, Some("B"), "De");
        assert!(result.is_err(), "extension is blocked, got {:?}", result);
    }

    #[test]
    fn restriction_allowed() {
        let s = schema();
        let result = resolve_xsi_type(&s, Some("B"), "Dr");
        assert_eq!(result.as_deref(), Ok("Dr"));
    }

    #[test]
    fn unknown_type_rejected() {
        let s = schema();
        assert!(resolve_xsi_type(&s, Some("B"), "NoSuchType").is_err());
    }

    #[test]
    fn same_type_allowed() {
        let s = schema();
        assert_eq!(resolve_xsi_type(&s, Some("B"), "B").as_deref(), Ok("B"));
    }
}
