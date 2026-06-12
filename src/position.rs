//! Position tracking reader for accurate line/column reporting.
//!
//! Provides a wrapper reader that tracks line and column numbers as bytes are read.

use std::io::{self, BufRead, Read};

/// A reader wrapper that tracks position (line, column, byte offset) while reading.
///
/// This is useful for providing accurate error locations during XML parsing.
/// Column numbers are counted in UTF-8 characters (not bytes), so multi-byte
/// characters like Japanese are counted as single columns.
///
/// # Example
///
/// ```
/// use std::io::BufRead;
/// use fastxml::position::PositionTrackingReader;
///
/// let input = b"line1\nline2";
/// let mut reader = PositionTrackingReader::new(&input[..]);
///
/// // Read first line
/// let mut buf = String::new();
/// reader.read_line(&mut buf).unwrap();
///
/// // Position is now at start of line 2
/// assert_eq!(reader.line(), 2);
/// assert_eq!(reader.column(), 1);
/// ```
pub struct PositionTrackingReader<R> {
    inner: R,
    line: usize,
    column: usize,
    byte_offset: usize,
}

impl<R> PositionTrackingReader<R> {
    /// Creates a new position tracking reader.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line: 1,
            column: 1,
            byte_offset: 0,
        }
    }

    /// Returns the current line number (1-indexed).
    pub fn line(&self) -> usize {
        self.line
    }

    /// Returns the current column number (1-indexed, in UTF-8 characters).
    pub fn column(&self) -> usize {
        self.column
    }

    /// Returns the current byte offset from the start.
    pub fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    /// Returns the inner reader, consuming this wrapper.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Returns a reference to the inner reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Returns a mutable reference to the inner reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Updates position tracking for consumed bytes.
    fn track_bytes(&mut self, bytes: &[u8]) {
        track(
            bytes,
            &mut self.line,
            &mut self.column,
            &mut self.byte_offset,
        );
    }
}

/// Bulk position update for a consumed chunk.
///
/// Lines are counted by scanning for `\n` with memchr; columns count UTF-8
/// characters as "bytes that are not continuation bytes", which is correct
/// for valid UTF-8 and works across chunk boundaries without buffering
/// partial sequences.
fn track(bytes: &[u8], line: &mut usize, column: &mut usize, byte_offset: &mut usize) {
    *byte_offset += bytes.len();
    let count_chars = |b: &[u8]| b.iter().filter(|&&x| x & 0xC0 != 0x80).count();
    match memchr::memrchr(b'\n', bytes) {
        Some(last_nl) => {
            *line += memchr::memchr_iter(b'\n', bytes).count();
            *column = 1 + count_chars(&bytes[last_nl + 1..]);
        }
        None => *column += count_chars(bytes),
    }
}

impl<R: Read> Read for PositionTrackingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.track_bytes(&buf[..n]);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for PositionTrackingReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        // Borrow the buffered bytes and the position fields disjointly so the
        // consumed chunk can be scanned without copying it out.
        let Self {
            inner,
            line,
            column,
            byte_offset,
        } = self;
        if let Ok(buf) = inner.fill_buf() {
            track(&buf[..amt.min(buf.len())], line, column, byte_offset);
        }
        inner.consume(amt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_basic_tracking() {
        let input = b"abc\ndef";
        let mut reader = PositionTrackingReader::new(Cursor::new(&input[..]));

        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 1);
        assert_eq!(reader.byte_offset(), 0);

        let mut buf = [0u8; 3];
        reader.read_exact(&mut buf).unwrap();

        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 4); // After "abc"
        assert_eq!(reader.byte_offset(), 3);

        reader.read_exact(&mut buf[..1]).unwrap(); // Read '\n'
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 1);
    }

    #[test]
    fn test_utf8_tracking() {
        // "あいう\nえお" - 3 Japanese chars, newline, 2 Japanese chars
        let input = "あいう\nえお";
        let mut reader = PositionTrackingReader::new(Cursor::new(input.as_bytes()));

        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 1);

        // Read "あ" (3 bytes)
        let mut buf = [0u8; 3];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 2); // 1 character read

        // Read "い" (3 bytes)
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 3);

        // Read "う" (3 bytes)
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 4);

        // Read newline
        let mut buf = [0u8; 1];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 1);

        // Read "え" (3 bytes)
        let mut buf = [0u8; 3];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 2);
    }

    #[test]
    fn test_bufread_consume() {
        let input = b"line1\nline2\nline3";
        let mut reader = PositionTrackingReader::new(Cursor::new(&input[..]));

        // Use BufRead interface
        let buf = reader.fill_buf().unwrap();
        assert!(buf.len() >= 5);

        reader.consume(5); // Consume "line1"
        assert_eq!(reader.line(), 1);
        assert_eq!(reader.column(), 6);
        assert_eq!(reader.byte_offset(), 5);

        reader.consume(1); // Consume '\n'
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 1);
        assert_eq!(reader.byte_offset(), 6);
    }

    #[test]
    fn test_read_line() {
        let input = b"first\nsecond\nthird";
        let mut reader = PositionTrackingReader::new(Cursor::new(&input[..]));

        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "first\n");
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 1);

        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "second\n");
        assert_eq!(reader.line(), 3);
        assert_eq!(reader.column(), 1);
    }

    #[test]
    fn test_mixed_ascii_utf8() {
        // Mixed ASCII and Japanese
        let input = "ab\nあい\nxy";
        let mut reader = PositionTrackingReader::new(Cursor::new(input.as_bytes()));

        let mut buf = String::new();

        // Read "ab\n"
        reader.read_line(&mut buf).unwrap();
        assert_eq!(reader.line(), 2);
        assert_eq!(reader.column(), 1);
        assert_eq!(reader.byte_offset(), 3);

        buf.clear();
        // Read "あい\n" (6 bytes for chars + 1 for newline)
        reader.read_line(&mut buf).unwrap();
        assert_eq!(reader.line(), 3);
        assert_eq!(reader.column(), 1);
        assert_eq!(reader.byte_offset(), 10); // 3 + 6 + 1
    }
}
