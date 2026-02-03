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
    /// Buffer for incomplete UTF-8 sequences
    utf8_buf: [u8; 4],
    utf8_len: usize,
}

impl<R> PositionTrackingReader<R> {
    /// Creates a new position tracking reader.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line: 1,
            column: 1,
            byte_offset: 0,
            utf8_buf: [0; 4],
            utf8_len: 0,
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
        for &byte in bytes {
            self.byte_offset += 1;

            // Handle UTF-8 multi-byte sequences
            if self.utf8_len > 0 {
                // We're in the middle of a multi-byte sequence
                self.utf8_buf[self.utf8_len] = byte;
                self.utf8_len += 1;

                // Check if we have a complete character
                if let Ok(s) = std::str::from_utf8(&self.utf8_buf[..self.utf8_len]) {
                    if !s.is_empty() {
                        let ch = s.chars().next().unwrap();
                        if ch == '\n' {
                            self.line += 1;
                            self.column = 1;
                        } else {
                            self.column += 1;
                        }
                        self.utf8_len = 0;
                    }
                } else if self.utf8_len >= 4 {
                    // Invalid UTF-8, reset and count as one character
                    self.column += 1;
                    self.utf8_len = 0;
                }
            } else if byte & 0x80 == 0 {
                // ASCII byte
                if byte == b'\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
            } else if byte & 0xC0 == 0xC0 {
                // Start of multi-byte sequence
                self.utf8_buf[0] = byte;
                self.utf8_len = 1;
            } else {
                // Continuation byte without start - count as character
                self.column += 1;
            }
        }
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
        // Get the bytes before consuming - need to copy to avoid borrow issues
        let bytes_to_track: Vec<u8> = if let Ok(buf) = self.inner.fill_buf() {
            let track_amt = amt.min(buf.len());
            buf[..track_amt].to_vec()
        } else {
            Vec::new()
        };
        self.track_bytes(&bytes_to_track);
        self.inner.consume(amt);
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
