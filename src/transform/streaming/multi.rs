//! Multi-handler streaming processing functions.

use std::collections::HashMap;
use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::Event;

use super::super::editable::EditableNodeBuilder;
use super::super::error::{TransformError, TransformResult};
use super::helpers::{
    PathTracker, add_empty_to_builder, add_end_to_builder, add_start_to_builder,
    extract_element_info, serialize_editable, xml_parse_error_with_location,
};
use super::{
    HandlerState, MultiHandler, MultiHandlerWithContext, MultiTransformHandler,
    MultiTransformHandlerWithContext, TransformHandlerState,
};

/// Processes XML with streaming iteration for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_for_each` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi<'a>(
    input: &str,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Initialize handler states
    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
            match_context: None,
        })
        .collect();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Already inside matched subtree, keep buffering
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // Match starts here, start buffering subtree
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        states[i].builder = Some(builder);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Inside matched subtree
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // This empty element is a match
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_empty_to_builder(&mut builder, &e, namespaces)?;

                        let mut editable = builder.build()?;
                        handlers[i].1(&mut editable);
                        match_count += 1;
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                // Check each handler for completed subtrees
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, call callback
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            match_count += 1;
                        } else {
                            // Not complete yet, put back
                            states[i].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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

/// Processes XML with streaming iteration and context for multiple XPath handlers in a single pass.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi_with_context<'a>(
    input: &str,
    handlers: &mut [MultiHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    // Initialize handler states
    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
            match_context: None,
        })
        .collect();

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Already inside matched subtree, keep buffering
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // Match starts here, capture context and start buffering subtree
                        states[i].match_context = Some(tracker.to_context());
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_start_to_builder(&mut builder, &e, namespaces)?;
                        states[i].builder = Some(builder);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                // Check each handler
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        // Inside matched subtree
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
                        // This empty element is a match
                        let ctx = tracker.to_context();
                        let mut builder = EditableNodeBuilder::new();
                        builder.set_namespaces(namespaces.clone());
                        add_empty_to_builder(&mut builder, &e, namespaces)?;

                        let mut editable = builder.build()?;
                        handlers[i].1(&mut editable, &ctx);
                        match_count += 1;
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                // Check each handler for completed subtrees
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, call callback
                            let mut editable = builder.build()?;
                            let ctx = states[i]
                                .match_context
                                .take()
                                .unwrap_or_else(|| tracker.to_context());
                            handlers[i].1(&mut editable, &ctx);
                            match_count += 1;
                        } else {
                            // Not complete yet, put back
                            states[i].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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

/// Processes XML with streaming transformation for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming` multiple times - the XML
/// is parsed only once and each element is checked against all XPath patterns.
///
/// # Strategy: First-Match-Wins
///
/// When multiple handlers could match nested elements, only the **first matching handler**
/// (in registration order) is active at a time. While a handler is processing an element's
/// subtree, other handlers cannot match elements within that subtree.
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();
    let mut prev_written: usize = 0;

    // Initialize handler states
    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
            match_context: None,
            match_start_offset: 0,
        })
        .collect();

    // Track which handler is currently active (first-match-wins strategy)
    let mut active_handler: Option<usize> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Already inside matched subtree, keep buffering
                    if let Some(ref mut builder) = states[idx].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // Match starts here!
                            // 1. Write everything before this point (zero-copy)
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            // 2. Start buffering subtree
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_start_to_builder(&mut builder, &e, namespaces)?;
                            states[i].builder = Some(builder);
                            states[i].match_start_offset = before_pos;
                            active_handler = Some(i);
                            break; // First match wins
                        }
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Inside matched subtree
                    if let Some(ref mut builder) = states[idx].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // This empty element is a match
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_empty_to_builder(&mut builder, &e, namespaces)?;

                            // Process immediately since it's complete
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            break; // First match wins
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(idx) = active_handler {
                    if let Some(mut builder) = states[idx].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, process it
                            let mut editable = builder.build()?;
                            handlers[idx].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            active_handler = None;
                        } else {
                            // Not complete yet, put back
                            states[idx].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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

/// Processes XML with streaming transformation and context for multiple XPath handlers in a single pass.
///
/// This is an optimization over calling `process_streaming_with_context` multiple times.
///
/// # Strategy: First-Match-Wins
///
/// Uses the same first-match-wins strategy as [`process_streaming_multi`].
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi_with_context<'a, W: Write>(
    input: &str,
    handlers: &mut [MultiTransformHandlerWithContext<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();
    let mut prev_written: usize = 0;

    // Initialize handler states
    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
            match_context: None,
            match_start_offset: 0,
        })
        .collect();

    // Track which handler is currently active (first-match-wins strategy)
    let mut active_handler: Option<usize> = None;

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Already inside matched subtree, keep buffering
                    if let Some(ref mut builder) = states[idx].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // Match starts here!
                            // 1. Write everything before this point (zero-copy)
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            // 2. Capture context at match point
                            states[i].match_context = Some(tracker.to_context());

                            // 3. Start buffering subtree
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_start_to_builder(&mut builder, &e, namespaces)?;
                            states[i].builder = Some(builder);
                            states[i].match_start_offset = before_pos;
                            active_handler = Some(i);
                            break; // First match wins
                        }
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    // Inside matched subtree
                    if let Some(ref mut builder) = states[idx].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    }
                } else {
                    // Check each handler for a match (first-match-wins)
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            // This empty element is a match
                            writer.write_all(&input.as_bytes()[prev_written..before_pos])?;

                            let ctx = tracker.to_context();

                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_empty_to_builder(&mut builder, &e, namespaces)?;

                            // Process immediately since it's complete
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable, &ctx);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            break; // First match wins
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(idx) = active_handler {
                    if let Some(mut builder) = states[idx].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            // Subtree complete, process it
                            let mut editable = builder.build()?;
                            let ctx = states[idx]
                                .match_context
                                .take()
                                .unwrap_or_else(|| tracker.to_context());
                            handlers[idx].1(&mut editable, &ctx);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, writer)?;
                            }

                            prev_written = after_pos;
                            active_handler = None;
                        } else {
                            // Not complete yet, put back
                            states[idx].builder = Some(builder);
                        }
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
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
