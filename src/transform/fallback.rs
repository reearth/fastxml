//! Two-pass fallback processor for non-streamable XPath expressions.
//!
//! When an XPath expression cannot be processed in a single streaming pass
//! (e.g., uses `last()`, backward axes, or complex predicates), this module
//! provides a two-pass approach:
//!
//! 1. **Pass 1**: Parse the document and evaluate XPath to find matching nodes
//! 2. **Pass 2**: Stream through the input, transforming matched nodes

use std::collections::HashSet;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::document::XmlDocument;
use crate::namespace::Namespace;
use crate::parser::parse;
use crate::serialize::{SerializeOptions, node_to_xml_string_with_options};
use crate::xpath::XPathResult;
use crate::xpath::evaluator::evaluate;

use super::editable::{EditableNode, EditableNodeBuilder};
use super::error::{TransformError, TransformResult};

/// Processes XML using two-pass fallback for non-streamable XPath.
pub fn process_fallback<W, F>(
    input: &str,
    xpath_expr: &str,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    // Pass 1: Parse document and evaluate XPath
    let doc = parse(input).map_err(|e| TransformError::XmlParse(e.to_string()))?;

    let result =
        evaluate(&doc, xpath_expr).map_err(|e| TransformError::InvalidXPath(e.to_string()))?;

    let matching_node_ids: HashSet<usize> = match result {
        XPathResult::Nodes(nodes) => nodes.iter().map(|n| n.id()).collect(),
        _ => {
            return Err(TransformError::InvalidXPath(
                "XPath must return nodes for transformation".to_string(),
            ));
        }
    };

    if matching_node_ids.is_empty() {
        // No matches, write input unchanged
        writer.write_all(input.as_bytes())?;
        return Ok(0);
    }

    // Pass 2: Stream through and transform matches
    process_with_matches(input, &doc, &matching_node_ids, &mut transform_fn, writer)
}

/// Second pass: stream through input, transforming matched nodes.
fn process_with_matches<W, F>(
    input: &str,
    doc: &XmlDocument,
    matching_ids: &HashSet<usize>,
    transform_fn: &mut F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    // Track current position in document node IDs
    // This is approximate - we match by structure
    let mut node_stack: Vec<usize> = vec![0]; // Start with document node
    let mut current_child_index: Vec<usize> = vec![0];

    let mut prev_written: usize = 0;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    // Track when we're inside a matched subtree
    let mut in_matched_subtree: Option<(usize, EditableNodeBuilder)> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                // Calculate expected node ID based on structure
                let parent_id = *node_stack.last().unwrap_or(&0);
                let child_idx = current_child_index.last().copied().unwrap_or(0);

                // Try to find matching node in document
                let expected_id = find_child_element_id(doc, parent_id, child_idx);

                if let Some((_, ref mut builder)) = in_matched_subtree {
                    // Already in matched subtree, add to builder
                    add_event_to_builder(builder, &Event::Start(e.clone()), input)?;
                } else if let Some(id) = expected_id {
                    if matching_ids.contains(&id) {
                        // Start of a matched node
                        writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                        let mut builder = EditableNodeBuilder::new();
                        add_event_to_builder(&mut builder, &Event::Start(e.clone()), input)?;
                        in_matched_subtree = Some((id, builder));
                    }
                }

                // Update stack
                if let Some(id) = expected_id {
                    node_stack.push(id);
                }
                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
                current_child_index.push(0);
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;

                let parent_id = *node_stack.last().unwrap_or(&0);
                let child_idx = current_child_index.last().copied().unwrap_or(0);
                let expected_id = find_child_element_id(doc, parent_id, child_idx);

                if let Some((_, ref mut builder)) = in_matched_subtree {
                    add_event_to_builder(builder, &Event::Empty(e.clone()), input)?;
                } else if let Some(id) = expected_id {
                    if matching_ids.contains(&id) {
                        // Matched empty element
                        writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                        let mut builder = EditableNodeBuilder::new();
                        add_event_to_builder(&mut builder, &Event::Empty(e.clone()), input)?;

                        let mut editable = builder.build()?;
                        transform_fn(&mut editable);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, writer)?;
                        }

                        prev_written = after_pos;
                    }
                }

                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some((id, mut builder)) = in_matched_subtree.take() {
                    add_event_to_builder(&mut builder, &Event::End(e.clone()), input)?;

                    if builder.is_complete() {
                        // Matched subtree complete
                        let mut editable = builder.build()?;
                        transform_fn(&mut editable);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, writer)?;
                        }

                        prev_written = after_pos;
                    } else {
                        // Not complete yet, put back
                        in_matched_subtree = Some((id, builder));
                    }
                }

                node_stack.pop();
                current_child_index.pop();
            }

            Ok(Event::Text(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                writer.write_all(&input.as_bytes()[prev_written..])?;
                break;
            }

            Ok(_) => {}

            Err(e) => {
                return Err(TransformError::XmlParse(format!(
                    "Error at position {}: {:?}",
                    reader.buffer_position(),
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

/// Find the nth child element ID of a parent node.
fn find_child_element_id(doc: &XmlDocument, parent_id: usize, child_index: usize) -> Option<usize> {
    let parent = doc.get_node(parent_id)?;
    let children = parent.get_child_elements();
    children.get(child_index).map(|n| n.id())
}

fn add_event_to_builder(
    builder: &mut EditableNodeBuilder,
    event: &Event,
    _input: &str,
) -> TransformResult<()> {
    match event {
        Event::Start(e) => {
            let name_bytes = e.name();
            let full_name =
                std::str::from_utf8(name_bytes.as_ref()).map_err(TransformError::Utf8)?;

            let (prefix, name) = match full_name.split_once(':') {
                Some((p, n)) => (Some(p), n),
                None => (None, full_name),
            };

            let mut attributes = Vec::new();
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
                    attributes.push((key.to_string(), value.to_string()));
                }
            }

            let attr_refs: Vec<(&str, &str)> = attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();

            builder.start_element(name, prefix, None, attr_refs, ns_decls);
        }

        Event::Empty(e) => {
            // Treat as start + end
            add_event_to_builder(builder, &Event::Start(e.to_owned()), _input)?;
            builder.end_element();
        }

        Event::End(_) => {
            builder.end_element();
        }

        _ => {}
    }

    Ok(())
}

fn serialize_editable<W: Write>(editable: &EditableNode, writer: &mut W) -> TransformResult<()> {
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

/// Processes XML for iteration without transformation (two-pass fallback).
pub fn process_for_each<F>(input: &str, xpath_expr: &str, mut callback: F) -> TransformResult<usize>
where
    F: FnMut(&EditableNode),
{
    // Pass 1: Parse document and evaluate XPath
    let doc = parse(input).map_err(|e| TransformError::XmlParse(e.to_string()))?;

    let result =
        evaluate(&doc, xpath_expr).map_err(|e| TransformError::InvalidXPath(e.to_string()))?;

    let matching_node_ids: HashSet<usize> = match result {
        XPathResult::Nodes(nodes) => nodes.iter().map(|n| n.id()).collect(),
        _ => {
            return Err(TransformError::InvalidXPath(
                "XPath must return nodes for iteration".to_string(),
            ));
        }
    };

    if matching_node_ids.is_empty() {
        return Ok(0);
    }

    // Pass 2: Stream through and call callback for matches
    iterate_with_matches(input, &doc, &matching_node_ids, &mut callback)
}

/// Second pass: stream through input, calling callback for matched nodes.
fn iterate_with_matches<F>(
    input: &str,
    doc: &XmlDocument,
    matching_ids: &HashSet<usize>,
    callback: &mut F,
) -> TransformResult<usize>
where
    F: FnMut(&EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut node_stack: Vec<usize> = vec![0];
    let mut current_child_index: Vec<usize> = vec![0];
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    let mut in_matched_subtree: Option<(usize, EditableNodeBuilder)> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let parent_id = *node_stack.last().unwrap_or(&0);
                let child_idx = current_child_index.last().copied().unwrap_or(0);
                let expected_id = find_child_element_id(doc, parent_id, child_idx);

                if let Some((_, ref mut builder)) = in_matched_subtree {
                    add_event_to_builder(builder, &Event::Start(e.clone()), input)?;
                } else if let Some(id) = expected_id {
                    if matching_ids.contains(&id) {
                        let mut builder = EditableNodeBuilder::new();
                        add_event_to_builder(&mut builder, &Event::Start(e.clone()), input)?;
                        in_matched_subtree = Some((id, builder));
                    }
                }

                if let Some(id) = expected_id {
                    node_stack.push(id);
                }
                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
                current_child_index.push(0);
            }

            Ok(Event::Empty(e)) => {
                let parent_id = *node_stack.last().unwrap_or(&0);
                let child_idx = current_child_index.last().copied().unwrap_or(0);
                let expected_id = find_child_element_id(doc, parent_id, child_idx);

                if let Some((_, ref mut builder)) = in_matched_subtree {
                    add_event_to_builder(builder, &Event::Empty(e.clone()), input)?;
                } else if let Some(id) = expected_id {
                    if matching_ids.contains(&id) {
                        let mut builder = EditableNodeBuilder::new();
                        add_event_to_builder(&mut builder, &Event::Empty(e.clone()), input)?;

                        let editable = builder.build()?;
                        callback(&editable);
                        match_count += 1;
                    }
                }

                if let Some(idx) = current_child_index.last_mut() {
                    *idx += 1;
                }
            }

            Ok(Event::End(e)) => {
                if let Some((id, mut builder)) = in_matched_subtree.take() {
                    add_event_to_builder(&mut builder, &Event::End(e.clone()), input)?;

                    if builder.is_complete() {
                        let editable = builder.build()?;
                        callback(&editable);
                        match_count += 1;
                    } else {
                        in_matched_subtree = Some((id, builder));
                    }
                }

                node_stack.pop();
                current_child_index.pop();
            }

            Ok(Event::Text(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some((_, ref mut builder)) = in_matched_subtree {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                return Err(TransformError::XmlParse(format!(
                    "Error at position {}: {:?}",
                    reader.buffer_position(),
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_with_last() {
        let input = r#"<root><item>A</item><item>B</item><item>C</item></root>"#;

        let mut output = Vec::new();
        let count = process_fallback(
            input,
            "//item[last()]",
            |node| {
                node.set_attribute("last", "true");
            },
            &mut output,
        )
        .unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 1);
        // The last item should have the attribute
        assert!(result.contains(r#"last="true""#));
    }

    #[test]
    fn test_fallback_no_match() {
        let input = r#"<root><item>A</item></root>"#;

        let mut output = Vec::new();
        let count = process_fallback(input, "//nonexistent", |_node| {}, &mut output).unwrap();

        let result = String::from_utf8(output).unwrap();
        assert_eq!(count, 0);
        assert_eq!(result, input);
    }
}
