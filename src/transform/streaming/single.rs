//! Single-handler streaming processing functions.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::super::context::TransformContext;
use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    PathTracker, add_empty_to_builder, add_end_to_builder, add_start_to_builder,
    extract_element_info, serialize_editable, xml_parse_error_with_location,
};

/// Processes XML with streaming transformation.
pub fn process_streaming<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut prev_written: usize = 0;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here!
                    // 1. Write everything before this point (zero-copy)
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    // 2. Start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    // Process immediately since it's complete
                    let mut editable = builder.build()?;
                    transform_fn(&mut editable);
                    transform_count += 1;

                    if !editable.is_removed() {
                        serialize_editable(&editable, writer)?;
                    }

                    prev_written = after_pos;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, process it
                        let mut editable = builder.build()?;
                        transform_fn(&mut editable);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, writer)?;
                        }

                        prev_written = after_pos;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                // Write remaining (zero-copy)
                writer.write_all(&input.as_bytes()[prev_written..])?;
                break;
            }

            Ok(_) => {
                // PI, Decl, DocType - pass through (handled by writing remaining)
            }

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

/// Processes XML with streaming transformation and context.
pub fn process_streaming_with_context<W, F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    W: Write,
    F: FnMut(&mut EditableNode, &TransformContext),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut prev_written: usize = 0;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    // Store context at match start for use when processing complete
    let mut match_context: Option<TransformContext> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here!
                    // 1. Write everything before this point (zero-copy)
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    // 2. Capture context at match point
                    match_context = Some(tracker.to_context());

                    // 3. Start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                    let ctx = tracker.to_context();

                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    // Process immediately since it's complete
                    let mut editable = builder.build()?;
                    transform_fn(&mut editable, &ctx);
                    transform_count += 1;

                    if !editable.is_removed() {
                        serialize_editable(&editable, writer)?;
                    }

                    prev_written = after_pos;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, process it
                        let mut editable = builder.build()?;
                        let ctx = match_context.take().unwrap_or_else(|| tracker.to_context());
                        transform_fn(&mut editable, &ctx);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, writer)?;
                        }

                        prev_written = after_pos;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                // Write remaining (zero-copy)
                writer.write_all(&input.as_bytes()[prev_written..])?;
                break;
            }

            Ok(_) => {
                // PI, Decl, DocType - pass through (handled by writing remaining)
            }

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

/// Processes XML with streaming iteration (no transformation output).
pub fn process_for_each<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here, start buffering subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    let mut editable = builder.build()?;
                    callback(&mut editable);
                    match_count += 1;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, call callback
                        let mut editable = builder.build()?;
                        callback(&mut editable);
                        match_count += 1;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML with streaming iteration and context (no transformation output).
pub fn process_for_each_with_context<F>(
    input: &str,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    F: FnMut(&mut EditableNode, &TransformContext),
{
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Store context at match start for use when processing complete
    let mut match_context: Option<TransformContext> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Already inside matched subtree, keep buffering
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts here, capture context
                    match_context = Some(tracker.to_context());
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    subtree_builder = Some(builder);
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    // Inside matched subtree
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // This empty element is a match
                    let ctx = tracker.to_context();
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, &e, namespaces)?;

                    let mut editable = builder.build()?;
                    callback(&mut editable, &ctx);
                    match_count += 1;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, &e)?;

                    if builder.is_complete() {
                        // Subtree complete, call callback
                        let mut editable = builder.build()?;
                        let ctx = match_context.take().unwrap_or_else(|| tracker.to_context());
                        callback(&mut editable, &ctx);
                        match_count += 1;
                    } else {
                        // Not complete yet, put back
                        subtree_builder = Some(builder);
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    builder.text(&text);
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.cdata(text);
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(ref mut builder) = subtree_builder {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    builder.comment(text);
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(_) => {}

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(xml_parse_error_with_location(
                    format!("{:?}", e),
                    byte_offset,
                    input,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}
