//! The single event loop behind every streaming transform (write) engine.
//!
//! In-memory and reader input, one handler or several, with or without a
//! [`TransformContext`]: all of them run through [`rewrite`]. The input
//! source decides only how unmatched content reaches the output (see
//! [`Output`]): in-memory input is copied verbatim by byte range, reader
//! input is echoed event by event.
//!
//! Only one match is open at a time. While a matched subtree is being read,
//! no handler can match inside it; at a start tag outside every match, the
//! first handler (in registration order) whose XPath matches takes it.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use quick_xml::Reader;
use quick_xml::events::Event;
use quick_xml::writer::Writer as XmlWriter;

use super::super::context::TransformContext;
use super::super::editable::{EditableNode, EditableNodeBuilder};
use super::super::error::{TransformError, TransformResult};
use super::super::xpath_analyze::StreamableXPath;
use super::helpers::{
    add_empty_to_builder, add_end_to_builder, add_pi_to_builder, add_start_to_builder, copy_bom,
    extract_element_info, serialize_editable, split_bom, xml_parse_error_at_offset,
    xml_parse_error_with_location,
};
use super::tracker::PathTracker;

/// What a handler does with a matched subtree before it is written back.
pub(crate) enum RewriteCallback<'h> {
    Node(&'h mut dyn FnMut(&mut EditableNode)),
    /// Like [`Node`](Self::Node), with the context at the match's start tag.
    NodeWithContext(&'h mut dyn FnMut(&mut EditableNode, &TransformContext)),
}

/// One handler: an XPath and its callback.
pub(crate) struct RewriteHandler<'h> {
    pub xpath: &'h StreamableXPath,
    pub callback: RewriteCallback<'h>,
}

impl RewriteHandler<'_> {
    fn wants_context(&self) -> bool {
        matches!(self.callback, RewriteCallback::NodeWithContext(_))
    }

    /// Calls the callback; `context` is the one captured at the match's
    /// start tag, if any, else the tracker's current one.
    fn call(
        &mut self,
        node: &mut EditableNode,
        context: Option<TransformContext>,
        tracker: &PathTracker,
    ) {
        match &mut self.callback {
            RewriteCallback::Node(f) => f(node),
            RewriteCallback::NodeWithContext(f) => {
                let ctx = context.unwrap_or_else(|| tracker.to_context());
                f(node, &ctx);
            }
        }
    }
}

/// Rewrites in-memory `input` to `writer`. Unmatched regions are copied
/// verbatim (zero-copy); a leading BOM is written first.
pub(crate) fn rewrite_str<W: Write>(
    input: &str,
    handlers: &mut [RewriteHandler<'_>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    let (bom, input) = split_bom(input);
    writer.write_all(bom.as_bytes())?;
    let mut output = CopyOutput {
        input,
        written: 0,
        writer,
    };
    rewrite(Reader::from_str(input), &mut output, handlers, namespaces)
}

/// Rewrites the XML read from `reader` to `writer`. Unmatched events are
/// echoed through quick-xml's writer; a leading BOM is copied first.
pub(crate) fn rewrite_reader<R: BufRead, W: Write>(
    mut reader: R,
    handlers: &mut [RewriteHandler<'_>],
    namespaces: &HashMap<String, String>,
    writer: &mut W,
) -> TransformResult<usize> {
    copy_bom(&mut reader, writer)?;
    let mut output = EchoOutput {
        writer: XmlWriter::new(writer),
    };
    rewrite(
        Reader::from_reader(reader),
        &mut output,
        handlers,
        namespaces,
    )
}

/// How the input reaches the output, around the matched subtrees.
trait Output {
    /// A match begins at byte offset `start`.
    fn match_start(&mut self, start: usize) -> TransformResult<()>;
    /// A match that ended at byte offset `end` was edited into `node`.
    fn match_end(&mut self, node: &EditableNode, end: usize) -> TransformResult<()>;
    /// An element, text, CDATA, comment or PI event outside every match.
    fn unmatched(&mut self, event: Event<'_>) -> TransformResult<()>;
    /// Any other event (declaration, doctype, …), inside a match or not.
    fn other(&mut self, event: Event<'_>) -> TransformResult<()>;
    /// The input is exhausted.
    fn finish(&mut self) -> TransformResult<()>;
    /// A parse error at byte offset `byte_offset`, inside `xpath`.
    fn parse_error(&self, message: String, byte_offset: usize, xpath: String) -> TransformError;
}

/// In-memory input: unmatched bytes are copied from `input` in ranges.
struct CopyOutput<'i, 'w, W> {
    /// The input after any BOM; quick-xml's offsets are relative to it.
    input: &'i str,
    /// End of the input already copied or replaced.
    written: usize,
    writer: &'w mut W,
}

impl<W: Write> Output for CopyOutput<'_, '_, W> {
    fn match_start(&mut self, start: usize) -> TransformResult<()> {
        self.writer
            .write_all(&self.input.as_bytes()[self.written..start])?;
        Ok(())
    }

    fn match_end(&mut self, node: &EditableNode, end: usize) -> TransformResult<()> {
        if !node.is_removed() {
            serialize_editable(node, self.writer)?;
        }
        self.written = end;
        Ok(())
    }

    fn unmatched(&mut self, _event: Event<'_>) -> TransformResult<()> {
        // Copied with the surrounding range.
        Ok(())
    }

    fn other(&mut self, _event: Event<'_>) -> TransformResult<()> {
        // Copied with the surrounding range.
        Ok(())
    }

    fn finish(&mut self) -> TransformResult<()> {
        self.writer
            .write_all(&self.input.as_bytes()[self.written..])?;
        Ok(())
    }

    fn parse_error(&self, message: String, byte_offset: usize, xpath: String) -> TransformError {
        xml_parse_error_with_location(message, byte_offset, self.input, Some(xpath))
    }
}

/// Reader input: unmatched events are re-serialized by quick-xml's writer.
struct EchoOutput<'w, W: Write> {
    writer: XmlWriter<&'w mut W>,
}

impl<W: Write> EchoOutput<'_, W> {
    fn echo(&mut self, event: Event<'_>) -> TransformResult<()> {
        self.writer
            .write_event(event)
            .map_err(|err| TransformError::Io(std::io::Error::other(err)))
    }
}

impl<W: Write> Output for EchoOutput<'_, W> {
    fn match_start(&mut self, _start: usize) -> TransformResult<()> {
        Ok(())
    }

    fn match_end(&mut self, node: &EditableNode, _end: usize) -> TransformResult<()> {
        if !node.is_removed() {
            serialize_editable(node, self.writer.get_mut())?;
        }
        Ok(())
    }

    fn unmatched(&mut self, event: Event<'_>) -> TransformResult<()> {
        self.echo(event)
    }

    fn other(&mut self, event: Event<'_>) -> TransformResult<()> {
        self.echo(event)
    }

    fn finish(&mut self) -> TransformResult<()> {
        Ok(())
    }

    fn parse_error(&self, message: String, byte_offset: usize, xpath: String) -> TransformError {
        xml_parse_error_at_offset(message, byte_offset, Some(xpath))
    }
}

/// The match being read.
struct OpenMatch {
    /// Index of the handler that took the match.
    handler: usize,
    builder: EditableNodeBuilder,
    /// Context at the match's start tag (context handlers only).
    context: Option<TransformContext>,
}

/// Runs `handlers` over the events of `reader` in a single pass, writing
/// through `output`. Returns the number of callback calls.
fn rewrite<R: BufRead, O: Output>(
    mut reader: Reader<R>,
    output: &mut O,
    handlers: &mut [RewriteHandler<'_>],
    namespaces: &HashMap<String, String>,
) -> TransformResult<usize> {
    reader.config_mut().trim_text(false);

    let mut tracker = PathTracker::for_xpaths(namespaces, handlers.iter().map(|h| h.xpath));
    let mut open: Option<OpenMatch> = None;
    let mut calls: usize = 0;
    let mut buf = Vec::new();

    // At a start tag outside every match: the first handler that matches.
    let first_match = |handlers: &[RewriteHandler<'_>], tracker: &PathTracker| {
        handlers.iter().position(|h| tracker.matches(h.xpath))
    };
    let new_builder = || {
        let mut builder = EditableNodeBuilder::new();
        builder.set_namespaces(namespaces.clone());
        builder
    };

    loop {
        let before_pos = reader.buffer_position() as usize;

        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                tracker.push_element(extract_element_info(&e, before_pos)?);

                if let Some(m) = &mut open {
                    add_start_to_builder(&mut m.builder, &e, namespaces)?;
                } else if let Some(idx) = first_match(handlers, &tracker) {
                    output.match_start(before_pos)?;
                    let context = handlers[idx].wants_context().then(|| tracker.to_context());
                    let mut builder = new_builder();
                    add_start_to_builder(&mut builder, &e, namespaces)?;
                    open = Some(OpenMatch {
                        handler: idx,
                        builder,
                        context,
                    });
                } else {
                    output.unmatched(Event::Start(e))?;
                }
            }

            Ok(Event::Empty(e)) => {
                let after_pos = reader.buffer_position() as usize;
                tracker.push_element(extract_element_info(&e, before_pos)?);

                if let Some(m) = &mut open {
                    add_empty_to_builder(&mut m.builder, &e, namespaces)?;
                } else if let Some(idx) = first_match(handlers, &tracker) {
                    // A complete match: edit and write it now.
                    output.match_start(before_pos)?;
                    let context = handlers[idx].wants_context().then(|| tracker.to_context());
                    let mut builder = new_builder();
                    add_empty_to_builder(&mut builder, &e, namespaces)?;
                    let mut node = builder.build()?;
                    handlers[idx].call(&mut node, context, &tracker);
                    calls += 1;
                    output.match_end(&node, after_pos)?;
                } else {
                    output.unmatched(Event::Empty(e))?;
                }

                tracker.pop_element();
            }

            Ok(Event::End(e)) => {
                let after_pos = reader.buffer_position() as usize;

                if let Some(mut m) = open.take() {
                    add_end_to_builder(&mut m.builder, &e)?;
                    if m.builder.is_complete() {
                        let mut node = m.builder.build()?;
                        handlers[m.handler].call(&mut node, m.context, &tracker);
                        calls += 1;
                        output.match_end(&node, after_pos)?;
                    } else {
                        open = Some(m);
                    }
                } else {
                    output.unmatched(Event::End(e))?;
                }

                tracker.pop_element();
            }

            Ok(Event::Text(e)) => {
                if let Some(m) = &mut open {
                    let text = e
                        .unescape()
                        .map_err(|err| TransformError::XmlParse(err.to_string()))?;
                    m.builder.text(&text);
                } else {
                    output.unmatched(Event::Text(e))?;
                }
            }

            Ok(Event::CData(e)) => {
                if let Some(m) = &mut open {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    m.builder.cdata(text);
                } else {
                    output.unmatched(Event::CData(e))?;
                }
            }

            Ok(Event::Comment(e)) => {
                if let Some(m) = &mut open {
                    let text = std::str::from_utf8(&e).map_err(TransformError::Utf8)?;
                    m.builder.comment(text);
                } else {
                    output.unmatched(Event::Comment(e))?;
                }
            }

            Ok(Event::PI(e)) => {
                if let Some(m) = &mut open {
                    add_pi_to_builder(&mut m.builder, &e)?;
                } else {
                    output.unmatched(Event::PI(e))?;
                }
            }

            Ok(Event::Eof) => {
                output.finish()?;
                break;
            }

            Ok(event) => output.other(event)?,

            Err(e) => {
                let byte_offset = reader.buffer_position() as usize;
                return Err(output.parse_error(
                    format!("{:?}", e),
                    byte_offset,
                    tracker.current_xpath(),
                ));
            }
        }

        buf.clear();
    }

    Ok(calls)
}
