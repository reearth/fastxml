//! End-of-line handling (XML 1.0 §2.11).
//!
//! Before parsing, a processor must translate every two-character sequence
//! CR LF, and every CR not followed by LF, into a single LF. [`EolNormalizer`]
//! does this on the byte stream below the tokenizer, so both parser loops see
//! normalized text, attribute values, comments, CDATA sections and DOCTYPE
//! declarations alike. A CR written as the character reference `&#13;` is
//! not touched (references are expanded later).
//!
//! The parsers count positions on the normalized stream: a lone CR counts as
//! a line break, and a byte offset reported after a CR LF is one less per
//! preceding CR LF than the offset in the original input.

use std::io::{self, BufRead, Read};

/// Most bytes normalized into the internal buffer per fill.
const NORMALIZE_CHUNK: usize = 64 * 1024;

/// The UTF-8 byte order mark.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// A [`BufRead`] adapter that turns CR LF and lone CR into LF.
///
/// Input without CR passes through without copying: the adapter hands out
/// the inner reader's buffer up to the next CR. From a CR on, up to 64 KiB
/// of the inner reader's current buffer is normalized into an internal
/// buffer in one pass, so input with CR LF line ends costs about one copy.
///
/// The first fill also holds a leading byte order mark in one piece:
/// quick-xml recognizes a BOM only when one fill holds all of it, and a
/// reader may return it in shorter reads.
pub(crate) struct EolNormalizer<R> {
    inner: R,
    /// Length of the prefix of the inner buffer known to contain no CR.
    clean: usize,
    /// Normalized bytes not yet consumed start at `pos`.
    normalized: Vec<u8>,
    pos: usize,
    /// The last byte normalized was a CR: an LF right after it is dropped.
    skip_lf: bool,
    /// Nothing has been read yet.
    at_start: bool,
}

impl<R> EolNormalizer<R> {
    /// Wraps `inner`.
    pub(crate) fn new(inner: R) -> Self {
        Self {
            inner,
            clean: 0,
            normalized: Vec::new(),
            pos: 0,
            skip_lf: false,
            at_start: true,
        }
    }
}

impl<R: BufRead> BufRead for EolNormalizer<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos < self.normalized.len() {
            return Ok(&self.normalized[self.pos..]);
        }
        if self.at_start {
            self.at_start = false;
            if self.join_split_bom()? {
                return Ok(&self.normalized);
            }
        }
        if self.clean == 0 {
            loop {
                let chunk = self.inner.fill_buf()?;
                if chunk.is_empty() {
                    return Ok(&[]);
                }
                if self.skip_lf {
                    self.skip_lf = false;
                    if chunk[0] == b'\n' {
                        self.inner.consume(1);
                        continue;
                    }
                }
                match memchr::memchr(b'\r', chunk) {
                    Some(0) => {
                        // Normalize up to NORMALIZE_CHUNK bytes of this chunk
                        // in one pass (bounded, so an in-memory input is not
                        // copied whole).
                        self.normalized.clear();
                        self.pos = 0;
                        let limit = chunk.len().min(NORMALIZE_CHUNK);
                        let mut i = 0;
                        while i < limit {
                            let rest = &chunk[i..limit];
                            let run = memchr::memchr(b'\r', rest).unwrap_or(rest.len());
                            self.normalized.extend_from_slice(&rest[..run]);
                            i += run;
                            if i < limit {
                                // chunk[i] is a CR
                                self.normalized.push(b'\n');
                                i += 1;
                                match chunk.get(i) {
                                    Some(b'\n') => i += 1,
                                    Some(_) => {}
                                    None => self.skip_lf = true,
                                }
                            }
                        }
                        self.inner.consume(i);
                        return Ok(&self.normalized);
                    }
                    Some(i) => self.clean = i,
                    None => self.clean = chunk.len(),
                }
                break;
            }
        }
        let clean = self.clean;
        let chunk = self.inner.fill_buf()?;
        Ok(&chunk[..clean.min(chunk.len())])
    }

    fn consume(&mut self, amt: usize) {
        if self.pos < self.normalized.len() {
            self.pos += amt;
            return;
        }
        self.clean = self.clean.saturating_sub(amt);
        self.inner.consume(amt);
    }
}

impl<R: BufRead> EolNormalizer<R> {
    /// When the first fill holds only the start of a BOM, reads the rest of
    /// it into the internal buffer. Returns whether it did.
    fn join_split_bom(&mut self) -> io::Result<bool> {
        let chunk = self.inner.fill_buf()?;
        if chunk.is_empty() || chunk.len() >= UTF8_BOM.len() || !UTF8_BOM.starts_with(chunk) {
            return Ok(false);
        }
        let mut head = Vec::with_capacity(UTF8_BOM.len());
        while head.len() < UTF8_BOM.len() {
            let chunk = self.inner.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            head.push(chunk[0]);
            self.inner.consume(1);
            if !UTF8_BOM.starts_with(&head) {
                break;
            }
        }
        // At most one byte here is not part of the BOM: a CR becomes an LF,
        // and an LF right after it is dropped on the next fill.
        if head.last() == Some(&b'\r') {
            *head.last_mut().expect("not empty") = b'\n';
            self.skip_lf = true;
        }
        self.normalized = head;
        self.pos = 0;
        Ok(true)
    }
}

impl<R: BufRead> Read for EolNormalizer<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(input: &[u8], capacity: usize) -> Vec<u8> {
        let mut r = EolNormalizer::new(io::BufReader::with_capacity(capacity, input));
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn translates_crlf_and_lone_cr() {
        for cap in [1, 2, 3, 8, 1024] {
            assert_eq!(
                normalize(b"a\r\nb\rc\r\r\nd\n\re\r", cap),
                b"a\nb\nc\n\nd\n\ne\n"
            );
        }
    }

    #[test]
    fn crlf_at_the_normalization_window_edge() {
        for pad in [NORMALIZE_CHUNK - 2, NORMALIZE_CHUNK - 1, NORMALIZE_CHUNK] {
            let mut input = b"\r".to_vec();
            input.extend(std::iter::repeat_n(b'a', pad));
            input.extend_from_slice(b"\r\nb\r");
            let mut r = EolNormalizer::new(&input[..]);
            let mut out = Vec::new();
            r.read_to_end(&mut out).unwrap();
            let mut expected = b"\n".to_vec();
            expected.extend(std::iter::repeat_n(b'a', pad));
            expected.extend_from_slice(b"\nb\n");
            assert_eq!(out, expected, "pad {pad}");
        }
    }

    #[test]
    fn first_fill_holds_a_bom_split_across_reads() {
        for cap in [1, 2] {
            let mut r =
                EolNormalizer::new(io::BufReader::with_capacity(cap, &b"\xEF\xBB\xBF<r/>"[..]));
            assert_eq!(r.fill_buf().unwrap(), UTF8_BOM, "capacity {cap}");
        }
    }

    #[test]
    fn a_split_bom_prefix_keeps_the_bytes_after_it() {
        for cap in [1, 2, 8] {
            assert_eq!(normalize(b"\xEF\r\n<", cap), b"\xEF\n<", "capacity {cap}");
            assert_eq!(normalize(b"\xEF\xBB<", cap), b"\xEF\xBB<", "capacity {cap}");
            assert_eq!(normalize(b"\xEF", cap), b"\xEF", "capacity {cap}");
        }
    }

    #[test]
    fn passes_input_without_cr_through() {
        assert_eq!(normalize(b"<r>a\nb</r>", 4), b"<r>a\nb</r>");
        let input = b"no carriage returns";
        let mut r = EolNormalizer::new(&input[..]);
        assert_eq!(r.fill_buf().unwrap(), input);
    }
}
