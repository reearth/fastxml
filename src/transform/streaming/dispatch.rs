//! The single event loop behind every streaming `for_each`.
//!
//! In-memory and reader input, one handler or several, with or without a
//! [`TransformContext`], editable subtrees ([`Transformer::on`]) or read-only
//! matches ([`Transformer::on_match`]): all of them run through [`dispatch`].
//! Each handler can stop on its own (a `limit`, or `ControlFlow::Break` from a
//! match callback); once every handler has stopped, reading stops.
//!
//! [`Transformer::on`]: crate::transform::Transformer::on
//! [`Transformer::on_match`]: crate::transform::Transformer::on_match

use std::collections::HashMap;
use std::io::BufRead;
use std::ops::ControlFlow;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::super::context::TransformContext;
use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::matched::ElementMatch;
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    add_empty_to_builder, add_end_to_builder, add_pi_to_builder, add_start_to_builder,
    extract_element_info, xml_parse_error_at_offset, xml_parse_error_with_location,
};
use super::tracker::PathTracker;

/// What a handler does with a match.
pub(crate) enum DispatchCallback<'h> {
    /// Receives the matched element's subtree as an editable node, once the
    /// subtree has been read. Matches nested inside it are not reported.
    Node(&'h mut dyn FnMut(&mut EditableNode)),
    /// Like [`Node`](Self::Node), with the context at the match's start tag.
    NodeWithContext(&'h mut dyn FnMut(&mut EditableNode, &TransformContext)),
    /// Receives the matched element's start tag only, as soon as it is read.
    /// Every match is reported, nested ones included.
    Match(&'h mut dyn FnMut(&ElementMatch) -> ControlFlow<()>),
}

/// One handler: an XPath, its callback, and how many calls it may make.
pub(crate) struct DispatchHandler<'h> {
    pub xpath: &'h StreamableXPath,
    pub callback: DispatchCallback<'h>,
    /// Maximum number of callback calls; `None` for no limit.
    pub limit: Option<usize>,
}

/// Where the events come from, for error locations: in-memory input can
/// report line and column, a reader only the byte offset.
#[derive(Clone, Copy)]
pub(crate) enum InputSource<'s> {
    Str(&'s str),
    Reader,
}

/// Per-handler progress through the document.
struct HandlerProgress {
    /// Subtree of the match being read (`Node` handlers only).
    builder: Option<EditableNodeBuilder>,
    /// Context at the start tag of the match being read.
    context: Option<TransformContext>,
    /// Calls left before the handler stops; `None` for no limit.
    remaining: Option<usize>,
    stopped: bool,
}

impl HandlerProgress {
    fn new(limit: Option<usize>) -> Self {
        Self {
            builder: None,
            context: None,
            remaining: limit,
            stopped: limit == Some(0),
        }
    }

    /// Records one callback call and its outcome.
    fn called(&mut self, flow: ControlFlow<()>) {
        if let Some(remaining) = &mut self.remaining {
            *remaining -= 1;
            if *remaining == 0 {
                self.stopped = true;
            }
        }
        if flow.is_break() {
            self.stopped = true;
        }
    }
}

/// Runs `handlers` over the events of `reader` in a single pass. Returns the
/// number of callback calls.
pub(crate) fn dispatch<R: BufRead>(
    mut reader: Reader<R>,
    source: InputSource<'_>,
    handlers: &mut [DispatchHandler<'_>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::for_xpaths(namespaces, handlers.iter().map(|h| h.xpath));
    let mut progress: Vec<HandlerProgress> = handlers
        .iter()
        .map(|h| HandlerProgress::new(h.limit))
        .collect();
    let mut calls: usize = 0;
    let mut buf = Vec::new();

    while !progress.iter().all(|p| p.stopped) {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                tracker.push_element(extract_element_info(&e, before_pos)?);
                calls += start_tag(&e, false, handlers, &mut progress, &tracker, namespaces)?;
            }

            Ok(Event::Empty(e)) => {
                tracker.push_element(extract_element_info(&e, before_pos)?);
                calls += start_tag(&e, true, handlers, &mut progress, &tracker, namespaces)?;
                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                for (handler, p) in handlers.iter_mut().zip(&mut progress) {
                    let Some(mut builder) = p.builder.take() else {
                        continue;
                    };
                    add_end_to_builder(&mut builder, &e)?;
                    if !builder.is_complete() {
                        p.builder = Some(builder);
                        continue;
                    }
                    let mut node = builder.build()?;
                    match &mut handler.callback {
                        DispatchCallback::Node(f) => f(&mut node),
                        DispatchCallback::NodeWithContext(f) => {
                            let ctx = p.context.take().unwrap_or_else(|| tracker.to_context());
                            f(&mut node, &ctx);
                        }
                        DispatchCallback::Match(_) => unreachable!("match handlers buffer nothing"),
                    }
                    calls += 1;
                    p.called(ControlFlow::Continue(()));
                }
                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if progress.iter().any(|p| p.builder.is_some()) {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    for builder in progress.iter_mut().filter_map(|p| p.builder.as_mut()) {
                        builder.text(&text);
                    }
                }
            }

            Ok(Event::CData(e)) => {
                if progress.iter().any(|p| p.builder.is_some()) {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    for builder in progress.iter_mut().filter_map(|p| p.builder.as_mut()) {
                        builder.cdata(text);
                    }
                }
            }

            Ok(Event::Comment(e)) => {
                if progress.iter().any(|p| p.builder.is_some()) {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    for builder in progress.iter_mut().filter_map(|p| p.builder.as_mut()) {
                        builder.comment(text);
                    }
                }
            }

            Ok(Event::PI(e)) => {
                for builder in progress.iter_mut().filter_map(|p| p.builder.as_mut()) {
                    add_pi_to_builder(builder, &e)?;
                }
            }

            Ok(Event::Eof) => break,

            Ok(_) => {}

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                let xpath = Some(tracker.current_xpath());
                return Err(match source {
                    InputSource::Str(input) => {
                        xml_parse_error_with_location(format!("{:?}", e), byte_offset, input, xpath)
                    }
                    InputSource::Reader => {
                        xml_parse_error_at_offset(format!("{:?}", e), byte_offset, xpath)
                    }
                });
            }
        }

        buf.clear();
    }

    Ok(calls)
}

/// Handles a start tag (or an empty-element tag when `empty`) already pushed
/// onto `tracker`. Returns the number of callback calls made.
fn start_tag(
    e: &BytesStart<'_>,
    empty: bool,
    handlers: &mut [DispatchHandler<'_>],
    progress: &mut [HandlerProgress],
    tracker: &PathTracker,
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    let mut calls = 0;
    // Built on the first match handler that matches, then shared.
    let mut element_match: Option<ElementMatch> = None;

    for (handler, p) in handlers.iter_mut().zip(progress.iter_mut()) {
        // A stopped subtree handler never has a subtree in progress: it
        // stops only after a callback, i.e. after its subtree was complete.
        if p.stopped {
            continue;
        }
        match &mut handler.callback {
            DispatchCallback::Match(f) => {
                if !tracker.matches(handler.xpath) {
                    continue;
                }
                if element_match.is_none() {
                    element_match = Some(element_match_of(e, tracker)?);
                }
                let flow = f(element_match.as_ref().expect("built above"));
                calls += 1;
                p.called(flow);
            }
            DispatchCallback::Node(_) | DispatchCallback::NodeWithContext(_) => {
                if let Some(builder) = &mut p.builder {
                    // Inside this handler's match: part of its subtree.
                    if empty {
                        add_empty_to_builder(builder, e, namespaces)?;
                    } else {
                        add_start_to_builder(builder, e, namespaces)?;
                    }
                    continue;
                }
                if !tracker.matches(handler.xpath) {
                    continue;
                }
                let mut builder = EditableNodeBuilder::new();
                builder.set_namespaces(namespaces.clone());
                if !empty {
                    add_start_to_builder(&mut builder, e, namespaces)?;
                    if matches!(handler.callback, DispatchCallback::NodeWithContext(_)) {
                        p.context = Some(tracker.to_context());
                    }
                    p.builder = Some(builder);
                    continue;
                }
                add_empty_to_builder(&mut builder, e, namespaces)?;
                let mut node = builder.build()?;
                match &mut handler.callback {
                    DispatchCallback::Node(f) => f(&mut node),
                    DispatchCallback::NodeWithContext(f) => f(&mut node, &tracker.to_context()),
                    DispatchCallback::Match(_) => unreachable!("handled above"),
                }
                calls += 1;
                p.called(ControlFlow::Continue(()));
            }
        }
    }

    Ok(calls)
}

/// The read-only view of the element whose start tag is `e`, the current
/// element of `tracker`.
fn element_match_of(e: &BytesStart<'_>, tracker: &PathTracker) -> TransformResult<ElementMatch> {
    let qname = std::str::from_utf8(e.name().as_ref())
        .map_err(TransformError::Utf8)?
        .to_string();
    let namespace_uri = tracker
        .current()
        .and_then(|info| info.namespace_uri.clone());
    let mut attributes = Vec::new();
    for attr in e.attributes().filter_map(|a| a.ok()) {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(TransformError::Utf8)?;
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let value = attr
            .unescape_value()
            .map_err(|err| TransformError::XmlParse(err.to_string()))?;
        // An unprefixed attribute is in no namespace.
        let ns = key
            .split_once(':')
            .and_then(|(prefix, _)| tracker.resolve_in_scope(prefix))
            .map(str::to_string);
        attributes.push((key.to_string(), value.into_owned(), ns));
    }
    Ok(ElementMatch::new(qname, namespace_uri, attributes))
}
