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
    /// How to turn the bytes of the events read into `&str`.
    fn utf8(&self) -> Utf8<'i>;
}

/// Turns event bytes into `&str`.
///
/// An in-memory input that is valid UTF-8 is checked once as a whole, and
/// bytes that lie inside it, starting and ending on character boundaries,
/// are then valid UTF-8 without another scan. Any other bytes (a reader's
/// buffer, an entity's replacement text, an input that is not UTF-8) are
/// checked as before, so errors and their messages do not change.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Utf8<'i> {
    validated: Option<&'i str>,
}

impl<'i> Utf8<'i> {
    /// Checks every conversion.
    pub(crate) fn checked() -> Self {
        Self { validated: None }
    }

    /// Validates `input` once; bytes inside it then convert without a scan.
    fn for_input(input: &'i [u8]) -> Self {
        Self {
            validated: std::str::from_utf8(input).ok(),
        }
    }

    /// `bytes` as `&str`.
    #[inline]
    pub(crate) fn str<'a>(&self, bytes: &'a [u8]) -> Result<&'a str, std::str::Utf8Error> {
        if let Some(input) = self.validated {
            let base = input.as_ptr() as usize;
            let start = (bytes.as_ptr() as usize).wrapping_sub(base);
            if start <= input.len()
                && bytes.len() <= input.len() - start
                && input.is_char_boundary(start)
                && input.is_char_boundary(start + bytes.len())
            {
                // SAFETY: `bytes` is `input[start..start + len]`: it lies
                // within `input`, which is valid UTF-8, and starts and ends on
                // character boundaries, so it is valid UTF-8 too.
                return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
            }
        }
        std::str::from_utf8(bytes)
    }
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

    fn utf8(&self) -> Utf8<'i> {
        Utf8::checked()
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
    utf8: Utf8<'i>,
}

impl<'i> SliceSource<'i> {
    /// Reads `input`, whose line ends are already normalized.
    pub(crate) fn new(input: &'i [u8]) -> Self {
        Self {
            reader: Reader::from_reader(input),
            input,
            scanned: 0,
            position: Position::default(),
            utf8: Utf8::for_input(input),
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

    fn utf8(&self) -> Utf8<'i> {
        self.utf8
    }
}

/// Sets up the tokenizer the way the streaming parser reads: text untrimmed,
/// empty-element tags reported as a start and an end.
pub(crate) fn configure_streaming<R>(reader: &mut Reader<R>) {
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_skips_the_scan_only_inside_a_validated_input() {
        let input = "a日b".as_bytes();
        let utf8 = Utf8::for_input(input);
        // Slices on character boundaries convert.
        assert_eq!(utf8.str(&input[..1]), Ok("a"));
        assert_eq!(utf8.str(&input[1..4]), Ok("日"));
        assert_eq!(utf8.str(&input[4..]), Ok("b"));
        // A slice that splits a character is checked, and rejected.
        assert!(utf8.str(&input[1..3]).is_err());
        assert!(utf8.str(&input[2..]).is_err());
        // Bytes from elsewhere are checked.
        assert!(utf8.str(b"\xFF").is_err());
        assert_eq!(utf8.str("ü".as_bytes()), Ok("ü"));
    }

    #[test]
    fn utf8_checks_everything_for_an_invalid_input() {
        let input = b"ok \xFF";
        let utf8 = Utf8::for_input(input);
        assert_eq!(utf8.str(&input[..2]), Ok("ok"));
        assert!(utf8.str(&input[3..]).is_err());
    }
}
