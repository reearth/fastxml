//! Helper functions for XSD parsing.

use std::collections::HashMap;

use compact_str::CompactString;

use crate::error::Result;
use crate::event::XmlEvent;
use crate::schema::xsd::types::Occurs;

/// XSD namespace URI.
pub const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

/// Parses attributes from a start element event.
pub(super) fn parse_attributes(attrs: &[(&str, &str)]) -> HashMap<String, String> {
    attrs
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Parses and validates minOccurs/maxOccurs from attributes.
/// Returns (minOccurs, maxOccurs) or an error if invalid.
pub(super) fn parse_occurs(attrs: &HashMap<String, String>) -> Result<(Occurs, Occurs)> {
    use crate::schema::error::SchemaError;

    let min = if let Some(min_str) = attrs.get("minOccurs") {
        Occurs::parse(min_str).map_err(|e| SchemaError::InvalidOccurs { value: e })?
    } else {
        Occurs::default()
    };

    let max = if let Some(max_str) = attrs.get("maxOccurs") {
        Occurs::parse(max_str).map_err(|e| SchemaError::InvalidOccurs { value: e })?
    } else {
        Occurs::default()
    };

    // Validate minOccurs <= maxOccurs
    match (&min, &max) {
        (Occurs::Count(min_val), Occurs::Count(max_val)) if min_val > max_val => {
            return Err(SchemaError::MinOccursGreaterThanMaxOccurs {
                min: *min_val,
                max: *max_val,
            }
            .into());
        }
        _ => {}
    }

    Ok((min, max))
}

/// Parses a non-negative integer facet value with validation.
pub(super) fn parse_facet_length(name: &str, value: &str) -> Result<u32> {
    use crate::schema::error::SchemaError;

    // Check for negative values
    if value.starts_with('-') {
        return Err(SchemaError::InvalidFacetValue {
            facet: name.to_string(),
            value: value.to_string(),
            reason: "must be non-negative".to_string(),
        }
        .into());
    }
    value.parse::<u32>().map_err(|_| {
        SchemaError::InvalidFacetValue {
            facet: name.to_string(),
            value: value.to_string(),
            reason: "must be a non-negative integer".to_string(),
        }
        .into()
    })
}

/// Converts a quick_xml start event to our XmlEvent type.
pub(super) fn convert_start_event(
    e: &quick_xml::events::BytesStart<'_>,
    line: usize,
    column: usize,
) -> Result<XmlEvent> {
    let name_bytes = e.name().as_ref().to_vec();
    let full_name = std::str::from_utf8(&name_bytes)?;
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls = Vec::new();
    let mut attributes = Vec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let value = attr.unescape_value().map_err(|e| {
            crate::parse_error::ParseError::AttributeDecodeError {
                message: e.to_string(),
            }
        })?;

        if key == "xmlns" {
            namespace_decls.push(crate::namespace::Namespace::default_ns(value.as_ref()));
        } else if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            namespace_decls.push(crate::namespace::Namespace::new(ns_prefix, value.as_ref()));
        } else {
            attributes.push((
                CompactString::from(key),
                CompactString::from(value.as_ref()),
            ));
        }
    }

    Ok(XmlEvent::StartElement {
        name: name.into(),
        prefix: prefix.map(|p| p.into()),
        namespace: None,
        attributes,
        namespace_decls,
        line: Some(line),
        column: Some(column),
    })
}
