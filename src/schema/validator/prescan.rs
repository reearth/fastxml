//! Reads an instance document's schema hints ahead of streaming validation.
//!
//! Streaming validation with an auto-detected schema runs in three steps:
//! [`prescan`] reads the input up to and including the root start tag and
//! extracts the hints, the caller loads the hinted schema (sync or async,
//! outside any parser callback), and the ordinary streaming validator then
//! parses the whole input in one pass — the bytes the pre-scan consumed are
//! replayed in front of the unread rest, so the validator sees the input
//! exactly as it was and reports the same lines, columns and byte offsets.

use std::io::{BufRead, BufReader, Chain, Cursor, Read};

use crate::error::Result;
use crate::event::{RawEvent, StreamingParser};
use crate::schema::hints::SchemaHints;

/// The input to validate, from its first byte: the bytes the pre-scan
/// consumed followed by the unread rest of the reader.
pub(crate) type Replay<R> = Chain<Cursor<Vec<u8>>, R>;

/// What the pre-scan found.
pub(crate) struct Prescanned<R> {
    /// The root element's schema hints; `None` if the input has no root
    /// element.
    pub hints: Option<SchemaHints>,
    /// The whole input again, for the validating pass.
    pub replay: Replay<R>,
}

/// Parses the prolog and the root start tag of `reader` (with the same
/// well-formedness checks as a full parse, so a malformed prolog or a
/// document without a root element fails here with the same error) and
/// reads the schema hints from the root start tag.
pub(crate) fn prescan<R: BufRead>(mut reader: R) -> Result<Prescanned<R>> {
    let mut consumed = Vec::new();
    let mut hints = None;
    {
        let recording = Recording {
            inner: &mut reader,
            log: &mut consumed,
        };
        let mut parser = StreamingParser::new(BufReader::new(recording));
        // The only start tag the pre-scan dispatches is the root's.
        parser.parse_through_root_start(|event| {
            if let RawEvent::StartElement {
                attributes,
                namespace_decls,
                ..
            } = event
            {
                hints = Some(SchemaHints::from_start_tag(attributes, namespace_decls));
            }
            Ok(())
        })?;
    }
    Ok(Prescanned {
        hints,
        replay: Cursor::new(consumed).chain(reader),
    })
}

/// A reader that keeps a copy of every byte read through it.
struct Recording<'a, R> {
    inner: &'a mut R,
    log: &'a mut Vec<u8>,
}

impl<R: Read> Read for Recording<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.log.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_all(mut r: impl Read) -> Vec<u8> {
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn replay_reproduces_the_input() {
        let xml = format!(
            "<?xml version=\"1.0\"?>\n<!--{}-->\n<root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"a.xsd\"><x/></root>",
            "c".repeat(20_000)
        );
        let reader = BufReader::with_capacity(5, xml.as_bytes());
        let scanned = prescan(reader).unwrap();
        let hints = scanned.hints.unwrap();
        assert_eq!(hints.hints.len(), 1);
        assert_eq!(hints.hints[0].location, "a.xsd");
        assert_eq!(read_all(scanned.replay), xml.as_bytes());
    }

    #[test]
    fn root_without_hints_has_empty_hints() {
        let scanned = prescan("<root/>".as_bytes()).unwrap();
        assert!(scanned.hints.unwrap().is_empty());
        assert_eq!(read_all(scanned.replay), b"<root/>");
    }

    #[test]
    fn stops_after_the_root_start_tag() {
        // Content after the root start tag is not parsed by the pre-scan:
        // its error surfaces only in the validating pass.
        let xml = format!("<root>{}<a></b></root>", " ".repeat(64 * 1024));
        let scanned = prescan(xml.as_bytes()).unwrap();
        assert!(scanned.hints.is_some());
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(prescan("".as_bytes()).is_err());
        assert!(prescan("<!-- only -->".as_bytes()).is_err());
    }
}
