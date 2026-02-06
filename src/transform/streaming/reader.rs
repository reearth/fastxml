//! Reader-based streaming processing functions.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::writer::Writer as XmlWriter;

use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    PathTracker, add_empty_to_builder, add_end_to_builder, add_start_to_builder,
    extract_element_info, serialize_editable, xml_parse_error_at_offset,
};
use super::{HandlerState, MultiHandler, MultiTransformHandler, TransformHandlerState};

/// Processes XML from a reader with streaming iteration (no transformation output).
///
/// Unlike `process_for_each`, this function reads from a `BufRead` source instead of
/// requiring the entire XML string in memory. This is important for processing large
/// XML files without excessive memory usage.
pub fn process_for_each_from_reader<R, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut callback: F,
) -> TransformResult<usize>
where
    R: BufRead,
    F: FnMut(&mut EditableNode),
{
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = xml_reader.buffer_position() as usize;

        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    add_start_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
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
                    add_empty_to_builder(builder, &e, namespaces)?;
                } else if tracker.matches(xpath) {
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
                        let mut editable = builder.build()?;
                        callback(&mut editable);
                        match_count += 1;
                    } else {
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
                let byte_offset = xml_reader.buffer_position() as usize;
                return Err(xml_parse_error_at_offset(
                    format!("{:?}", e),
                    byte_offset,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML from a reader with streaming transformation.
///
/// Unlike `process_streaming`, this function reads from a `BufRead` source and uses
/// quick_xml's Writer to echo non-matched events, avoiding the need to keep the entire
/// XML input in memory. Matched elements are still built into a DOM subtree for editing.
///
/// Note: The output may have minor formatting differences from the original XML
/// (e.g., attribute quoting style) for non-matched content, since events are
/// reconstructed through the XML writer rather than copied verbatim.
pub fn process_streaming_from_reader<R, W, F>(
    reader: R,
    xpath: &StreamableXPath,
    namespaces: &HashMap<String, String>,
    mut transform_fn: F,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
    F: FnMut(&mut EditableNode),
{
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut xml_writer = XmlWriter::new(writer);

    let mut tracker = PathTracker::new();
    let mut subtree_builder: Option<EditableNodeBuilder> = None;
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    loop {
        let before_pos = xml_reader.buffer_position() as usize;

        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let element_info = extract_element_info(e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    add_start_to_builder(builder, e, namespaces)?;
                } else if tracker.matches(xpath) {
                    // Match starts - start building DOM subtree
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_start_to_builder(&mut builder, e, namespaces)?;
                    subtree_builder = Some(builder);
                } else {
                    // No match - echo event to writer
                    xml_writer
                        .write_event(Event::Start(e.clone()))
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(Event::Empty(ref e)) => {
                let element_info = extract_element_info(e, before_pos, namespaces)?;

                tracker.push_element(element_info);

                if let Some(ref mut builder) = subtree_builder {
                    add_empty_to_builder(builder, e, namespaces)?;
                } else if tracker.matches(xpath) {
                    let mut builder = EditableNodeBuilder::new();
                    builder.set_namespaces(namespaces.clone());
                    add_empty_to_builder(&mut builder, e, namespaces)?;

                    let mut editable = builder.build()?;
                    transform_fn(&mut editable);
                    transform_count += 1;

                    if !editable.is_removed() {
                        serialize_editable(&editable, xml_writer.get_mut())?;
                    }
                } else {
                    xml_writer
                        .write_event(Event::Empty(e.clone()))
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }

                tracker.pop_element();
            }

            Ok(Event::End(ref e)) => {
                if let Some(mut builder) = subtree_builder.take() {
                    add_end_to_builder(&mut builder, e)?;

                    if builder.is_complete() {
                        let mut editable = builder.build()?;
                        transform_fn(&mut editable);
                        transform_count += 1;

                        if !editable.is_removed() {
                            serialize_editable(&editable, xml_writer.get_mut())?;
                        }
                    } else {
                        subtree_builder = Some(builder);
                    }
                } else {
                    xml_writer
                        .write_event(Event::End(e.clone()))
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }

                tracker.pop_element();
            }

            Ok(ref event @ Event::Text(_)) => {
                if let Some(ref mut builder) = subtree_builder {
                    if let Event::Text(e) = event {
                        let text = e
                            .unescape()
                            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                        builder.text(&text);
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(ref event @ Event::CData(_)) => {
                if let Some(ref mut builder) = subtree_builder {
                    if let Event::CData(e) = event {
                        let text = std::str::from_utf8(e).map_err(TransformError::Utf8)?;
                        builder.cdata(text);
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(ref event @ Event::Comment(_)) => {
                if let Some(ref mut builder) = subtree_builder {
                    if let Event::Comment(e) = event {
                        let text = std::str::from_utf8(e).map_err(TransformError::Utf8)?;
                        builder.comment(text);
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(event) => {
                // PI, Decl, DocType - pass through
                xml_writer
                    .write_event(event)
                    .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
            }

            Err(e) => {
                let byte_offset = xml_reader.buffer_position() as usize;
                return Err(xml_parse_error_at_offset(
                    format!("{:?}", e),
                    byte_offset,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}

/// Processes XML from a reader with streaming iteration for multiple XPath handlers.
///
/// Reader-based version of `process_for_each_multi`. Reads from a `BufRead` source
/// instead of requiring the entire XML string in memory.
#[allow(clippy::needless_range_loop)]
pub fn process_for_each_multi_from_reader<'a, R>(
    reader: R,
    handlers: &mut [MultiHandler<'a>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize>
where
    R: BufRead,
{
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::new();
    let mut match_count: usize = 0;
    let mut buf = Vec::new();

    let mut states: Vec<HandlerState> = handlers
        .iter()
        .map(|(xpath, _)| HandlerState {
            xpath,
            builder: None,
            match_context: None,
        })
        .collect();

    loop {
        let before_pos = xml_reader.buffer_position() as usize;

        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let element_info = extract_element_info(&e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        add_start_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
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

                for i in 0..states.len() {
                    if let Some(ref mut builder) = states[i].builder {
                        add_empty_to_builder(builder, &e, namespaces)?;
                    } else if tracker.matches(states[i].xpath) {
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
                for i in 0..states.len() {
                    if let Some(mut builder) = states[i].builder.take() {
                        add_end_to_builder(&mut builder, &e)?;

                        if builder.is_complete() {
                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            match_count += 1;
                        } else {
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
                let byte_offset = xml_reader.buffer_position() as usize;
                return Err(xml_parse_error_at_offset(
                    format!("{:?}", e),
                    byte_offset,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(match_count)
}

/// Processes XML from a reader with streaming transformation for multiple XPath handlers.
///
/// Reader-based version of `process_streaming_multi`. Uses quick_xml's Writer to echo
/// non-matched events instead of zero-copy byte slicing, avoiding the need to keep
/// the entire XML input in memory.
#[allow(clippy::needless_range_loop)]
pub fn process_streaming_multi_from_reader<'a, R, W>(
    reader: R,
    handlers: &mut [MultiTransformHandler<'a>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize>
where
    R: BufRead,
    W: Write,
{
    let mut xml_reader = Reader::from_reader(reader);
    xml_reader.config_mut().trim_text(false);

    let mut xml_writer = XmlWriter::new(writer);

    let mut tracker = PathTracker::new();
    let mut transform_count: usize = 0;
    let mut buf = Vec::new();

    let mut states: Vec<TransformHandlerState> = handlers
        .iter()
        .map(|(xpath, _)| TransformHandlerState {
            xpath,
            builder: None,
            match_context: None,
            match_start_offset: 0,
        })
        .collect();

    let mut active_handler: Option<usize> = None;

    loop {
        let before_pos = xml_reader.buffer_position() as usize;

        match xml_reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let element_info = extract_element_info(e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        add_start_to_builder(builder, e, namespaces)?;
                    }
                } else {
                    let mut matched = false;
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_start_to_builder(&mut builder, e, namespaces)?;
                            states[i].builder = Some(builder);
                            states[i].match_start_offset = before_pos;
                            active_handler = Some(i);
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        xml_writer
                            .write_event(Event::Start(e.clone()))
                            .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                    }
                }
            }

            Ok(Event::Empty(ref e)) => {
                let element_info = extract_element_info(e, before_pos, namespaces)?;
                tracker.push_element(element_info);

                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        add_empty_to_builder(builder, e, namespaces)?;
                    }
                } else {
                    let mut matched = false;
                    for i in 0..states.len() {
                        if tracker.matches(states[i].xpath) {
                            let mut builder = EditableNodeBuilder::new();
                            builder.set_namespaces(namespaces.clone());
                            add_empty_to_builder(&mut builder, e, namespaces)?;

                            let mut editable = builder.build()?;
                            handlers[i].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, xml_writer.get_mut())?;
                            }
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        xml_writer
                            .write_event(Event::Empty(e.clone()))
                            .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                    }
                }

                tracker.pop_element();
            }

            Ok(Event::End(ref e)) => {
                if let Some(idx) = active_handler {
                    if let Some(mut builder) = states[idx].builder.take() {
                        add_end_to_builder(&mut builder, e)?;

                        if builder.is_complete() {
                            let mut editable = builder.build()?;
                            handlers[idx].1(&mut editable);
                            transform_count += 1;

                            if !editable.is_removed() {
                                serialize_editable(&editable, xml_writer.get_mut())?;
                            }

                            active_handler = None;
                        } else {
                            states[idx].builder = Some(builder);
                        }
                    }
                } else {
                    xml_writer
                        .write_event(Event::End(e.clone()))
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }

                tracker.pop_element();
            }

            Ok(ref event @ Event::Text(_)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        if let Event::Text(e) = event {
                            let text = e
                                .unescape()
                                .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                            builder.text(&text);
                        }
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(ref event @ Event::CData(_)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        if let Event::CData(e) = event {
                            let text = std::str::from_utf8(e).map_err(TransformError::Utf8)?;
                            builder.cdata(text);
                        }
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(ref event @ Event::Comment(_)) => {
                if let Some(idx) = active_handler {
                    if let Some(ref mut builder) = states[idx].builder {
                        if let Event::Comment(e) = event {
                            let text = std::str::from_utf8(e).map_err(TransformError::Utf8)?;
                            builder.comment(text);
                        }
                    }
                } else {
                    xml_writer
                        .write_event(event.clone())
                        .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
                }
            }

            Ok(Event::Eof) => {
                break;
            }

            Ok(event) => {
                xml_writer
                    .write_event(event)
                    .map_err(|err| TransformError::Io(std::io::Error::other(err)))?;
            }

            Err(e) => {
                let byte_offset = xml_reader.buffer_position() as usize;
                return Err(xml_parse_error_at_offset(
                    format!("{:?}", e),
                    byte_offset,
                    Some(tracker.current_xpath()),
                ));
            }
        }

        buf.clear();
    }

    Ok(transform_count)
}
