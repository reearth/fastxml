//! Where the parsers' events come from.
//!
//! A [`BufRead`] source is read through [`EolNormalizer`] and
//! [`PositionTrackingReader`], and quick-xml copies each event into a buffer.
//! An in-memory input has its line ends normalized once up front and is read
//! in place by [`SliceSource`], which also counts positions only when asked.
//! Both report the same events, positions and errors.

use std::borrow::Cow;
use std::io::{BufRead, Read};

use quick_xml::Reader;
use quick_xml::events::Event;

use super::eol::EolNormalizer;
use crate::position::{Position, PositionTrackingReader};

/// Wraps a reader for parsing: line ends normalized, positions tracked.
pub(crate) fn reader_source<R: BufRead>(
    reader: R,
) -> Reader<PositionTrackingReader<EolNormalizer<R>>> {
    Reader::from_reader(PositionTrackingReader::new(EolNormalizer::new(reader)))
}

/// Normalizes line ends in an in-memory input (XML 1.0 §2.11), copying only
/// when the input has a CR, so the common LF-only input is parsed in place.
pub(crate) fn normalize_line_ends(input: Cow<'_, [u8]>) -> std::io::Result<Cow<'_, [u8]>> {
    if memchr::memchr(b'\r', &input).is_none() {
        return Ok(input);
    }
    let mut normalized = Vec::with_capacity(input.len());
    EolNormalizer::new(input.as_ref()).read_to_end(&mut normalized)?;
    Ok(Cow::Owned(normalized))
}

/// Where a parser's events come from: a reader that copies each event
/// into a buffer, or an in-memory input read in place.
pub(crate) trait EventSource<'i> {
    /// Reads the next event; it borrows `buf` or the input itself.
    fn next_event<'b>(&mut self, buf: &'b mut Vec<u8>) -> quick_xml::Result<Event<'b>>
    where
        'i: 'b;
    /// Line and column just after the event last read.
    fn line_column(&mut self) -> (usize, usize);
    /// Byte position reported with a tokenizer error.
    fn error_position(&self) -> u64;
    /// Number of (normalized) bytes consumed so far.
    fn consumed(&self) -> u64;
}

impl<'i, R: BufRead> EventSource<'i> for Reader<PositionTrackingReader<EolNormalizer<R>>> {
    fn next_event<'b>(&mut self, buf: &'b mut Vec<u8>) -> quick_xml::Result<Event<'b>>
    where
        'i: 'b,
    {
        self.read_event_into(buf)
    }

    fn line_column(&mut self) -> (usize, usize) {
        let position = self.get_ref();
        (position.line(), position.column())
    }

    fn error_position(&self) -> u64 {
        self.buffer_position()
    }

    fn consumed(&self) -> u64 {
        self.get_ref().byte_offset() as u64
    }
}

/// An in-memory input with normalized line ends, read in place. Positions
/// are counted lazily over the bytes read since the last query, which gives
/// the same line and column as [`PositionTrackingReader`] counting each
/// consumed chunk.
pub(crate) struct SliceSource<'i> {
    reader: Reader<&'i [u8]>,
    input: &'i [u8],
    /// How much of `input` `position` accounts for.
    scanned: usize,
    position: Position,
}

impl<'i> SliceSource<'i> {
    /// Reads `input`, whose line ends are already normalized.
    pub(crate) fn new(input: &'i [u8]) -> Self {
        Self {
            reader: Reader::from_reader(input),
            input,
            scanned: 0,
            position: Position::default(),
        }
    }

    /// The tokenizer, for configuring it.
    pub(crate) fn reader_mut(&mut self) -> &mut Reader<&'i [u8]> {
        &mut self.reader
    }
}

impl<'i> EventSource<'i> for SliceSource<'i> {
    fn next_event<'b>(&mut self, _buf: &'b mut Vec<u8>) -> quick_xml::Result<Event<'b>>
    where
        'i: 'b,
    {
        self.reader.read_event()
    }

    fn line_column(&mut self) -> (usize, usize) {
        // The bytes consumed so far, BOM included: the reader advances its
        // slice past everything it read, which can be more than
        // `buffer_position` reports (a `<` read after text).
        let end = self.input.len() - self.reader.get_ref().len();
        if end > self.scanned {
            self.position.advance(&self.input[self.scanned..end]);
            self.scanned = end;
        }
        (self.position.line, self.position.column)
    }

    fn error_position(&self) -> u64 {
        self.reader.buffer_position()
    }

    fn consumed(&self) -> u64 {
        (self.input.len() - self.reader.get_ref().len()) as u64
    }
}

/// Sets up the tokenizer the way the streaming parser reads: text untrimmed,
/// empty-element tags reported as a start and an end.
pub(crate) fn configure_streaming<R>(reader: &mut Reader<R>) {
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
}
