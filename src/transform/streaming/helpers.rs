//! Helper types and functions for streaming processing.

use std::collections::HashMap;
use std::io::Write;

use crate::namespace::Namespace;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};

use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{ErrorLocation, TransformError, TransformResult};
use super::tracker::ElementInfo;
#[cfg(doc)]
use super::tracker::PathTracker;
use quick_xml::events::{BytesEnd, BytesStart};

/// Creates an XML parse error with location information.
pub(crate) fn xml_parse_error_with_location(
    message: impl Into<String>,
    byte_offset: usize,
    input: &str,
    xpath: Option<String>,
) -> TransformError {
    let mut location = ErrorLocation::from_offset_with_input(byte_offset, input);
    if let Some(path) = xpath {
        location = location.with_xpath(path);
    }
    TransformError::XmlParseWithLocation {
        message: message.into(),
        location,
    }
}

/// Creates an XML parse error with byte offset only (no input string for line calculation).
pub(crate) fn xml_parse_error_at_offset(
    message: impl Into<String>,
    byte_offset: usize,
    xpath: Option<String>,
) -> TransformError {
    let mut location = ErrorLocation::from_offset(byte_offset);
    if let Some(path) = xpath {
        location = location.with_xpath(path);
    }
    TransformError::XmlParseWithLocation {
        message: message.into(),
        location,
    }
}

/// Reads an element's name and attributes from a start tag.
///
/// `namespace_uri` is left unresolved; [`PathTracker::push_element`] resolves
/// it from the namespace declarations in scope in the document.
pub(crate) fn extract_element_info(
    e: &BytesStart,
    start_offset: usize,
) -> TransformResult<ElementInfo> {
    let name_bytes = e.name();
    let full_name = std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

    let (prefix, name) = match full_name.split_once(':') {
        Some((p, n)) => (Some(p.to_string()), n.to_string()),
        None => (None, full_name.to_string()),
    };

    let mut attributes = HashMap::new();
    for attr in e.attributes().filter_map(|a| a.ok()) {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
        let value = attr
            .unescape_value()
            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
        attributes.insert(key.to_string(), value.to_string());
    }

    Ok(ElementInfo {
        name,
        prefix,
        namespace_uri: None,
        attributes,
        start_offset,
    })
}

pub(crate) fn add_start_to_builder(
    builder: &mut EditableNodeBuilder,
    e: &BytesStart,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    let name_bytes = e.name();
    let full_name = std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

    let (prefix, name) = match full_name.split_once(':') {
        Some((p, n)) => (Some(p), n),
        None => (None, full_name),
    };

    let namespace_uri = prefix.and_then(|p| namespaces.get(p).map(|s| s.as_str()));

    let mut attributes = Vec::new();
    let mut attr_ns_info = Vec::new();
    let mut ns_decls = Vec::new();

    for attr in e.attributes().filter_map(|a| a.ok()) {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
        let value = attr
            .unescape_value()
            .map_err(|err| TransformError::XmlParse(err.to_string()))?;

        if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            ns_decls.push(Namespace::new(ns_prefix, value.as_ref()));
        } else if key == "xmlns" {
            ns_decls.push(Namespace::new("", value.as_ref()));
        } else {
            // Store attributes with local names only (libxml compatible)
            let (attr_prefix, local_name) = match key.split_once(':') {
                Some((p, local)) => (Some(p), local),
                None => (None, key),
            };
            attributes.push((local_name.to_string(), value.to_string()));
            if let Some(p) = attr_prefix {
                if let Some(uri) = namespaces.get(p) {
                    attr_ns_info.push((local_name.to_string(), p.to_string(), uri.clone()));
                }
            }
        }
    }

    // Convert to references for the builder
    let attr_refs: Vec<(&str, &str)> = attributes
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let attr_ns_refs: Vec<(&str, &str, &str)> = attr_ns_info
        .iter()
        .map(|(l, p, u)| (l.as_str(), p.as_str(), u.as_str()))
        .collect();

    builder.start_element(
        name,
        prefix,
        namespace_uri,
        attr_refs,
        attr_ns_refs,
        ns_decls,
    );

    Ok(())
}

pub(crate) fn add_empty_to_builder(
    builder: &mut EditableNodeBuilder,
    e: &BytesStart,
    namespaces: &HashMap<String, String>,
) -> TransformResult<()> {
    add_start_to_builder(builder, e, namespaces)?;
    builder.end_element();
    Ok(())
}

pub(crate) fn add_end_to_builder(
    builder: &mut EditableNodeBuilder,
    _e: &BytesEnd,
) -> TransformResult<()> {
    builder.end_element();
    Ok(())
}

pub(crate) fn serialize_editable<W: Write>(
    editable: &EditableNode,
    writer: &mut W,
) -> TransformResult<()> {
    let root = editable
        .document()
        .get_root_element()
        .map_err(|e| TransformError::Serialization(e.to_string()))?;

    let xml =
        node_to_xml_string_with_options(editable.document(), &root, &SerializeOptions::default())
            .map_err(|e| TransformError::Serialization(e.to_string()))?;

    writer.write_all(xml.as_bytes())?;
    Ok(())
}
