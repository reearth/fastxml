//! Per-event well-formedness checks shared by the DOM and streaming parsers.
//!
//! quick-xml is a lenient tokenizer: it happily emits events for input the XML
//! specification requires a processor to reject. Historically each of the two
//! event loops (the DOM builder in [`crate::parser`] and the streaming
//! [`drive_loop`](crate::event) ) repeated the same raw-byte checks inline at
//! every event arm. [`WellformedChecker`] owns those checks so the two loops
//! stay in lockstep: each loop constructs one checker and calls exactly one
//! method per event.
//!
//! Every check runs on the *raw*, pre-unescape bytes quick-xml hands us. That
//! is deliberate: a character reference such as `&#1;` (which denotes an illegal
//! character) must be judged by decoding the reference, not by inspecting the
//! literal ASCII `&#1;`, so the literal-character checks here never see it.
//! Reference legality is handled separately.

use quick_xml::events::BytesStart;

use super::error::ParseError;
use super::wellformed::{check_chars, check_name};
use crate::error::Result;

/// Owns the per-event well-formedness checks for one parse.
///
/// Construct one per document and call the matching method for each event the
/// tokenizer produces. Methods borrow `&mut self` because later well-formedness
/// rules (document-structure state, namespace scopes) accumulate state across
/// events.
#[derive(Debug, Default)]
pub(crate) struct WellformedChecker {}

impl WellformedChecker {
    /// Creates a checker for a fresh document.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Checks a start (or empty-element) tag: the element `Name` and every
    /// attribute's `Name` and raw value.
    pub(crate) fn start(&mut self, e: &BytesStart<'_>) -> Result<()> {
        let qname = std::str::from_utf8(e.name().into_inner())?;
        check_name(qname, "element name")?;
        for attr_result in e.attributes() {
            let attr = attr_result.map_err(|e| ParseError::AttributeError {
                message: e.to_string(),
            })?;
            let key = std::str::from_utf8(attr.key.into_inner())?;
            check_name(key, "attribute name")?;
            // Raw value: literal characters must satisfy the Char production;
            // `&#…;` references survive as plain ASCII and are judged elsewhere.
            check_chars(std::str::from_utf8(attr.value.as_ref())?, "attribute value")?;
        }
        Ok(())
    }

    /// Checks an end tag's `Name`.
    ///
    /// With `check_end_names` enabled (the default for both engines) the
    /// tokenizer guarantees this name equals the matching start tag's name,
    /// which [`start`](Self::start) already validated, so this is effectively a
    /// no-op that keeps the two loops symmetric.
    pub(crate) fn end(&mut self, qname: &str) -> Result<()> {
        check_name(qname, "element name")?;
        Ok(())
    }

    /// Checks raw text (character data).
    pub(crate) fn text(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "text content")?;
        Ok(())
    }

    /// Checks a CDATA section body.
    pub(crate) fn cdata(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "CDATA section")?;
        Ok(())
    }

    /// Checks a comment body.
    pub(crate) fn comment(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "comment")?;
        Ok(())
    }

    /// Checks a processing instruction's raw body (target plus data).
    pub(crate) fn pi(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "processing instruction")?;
        Ok(())
    }

    /// Checks an XML declaration's raw body.
    pub(crate) fn decl(&mut self, _raw: &str) -> Result<()> {
        Ok(())
    }

    /// Checks a `DOCTYPE` declaration's raw body (name, external id, and
    /// internal subset, exactly as the tokenizer delivered it).
    pub(crate) fn doctype(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "document type declaration")?;
        Ok(())
    }

    /// Called once at end of input.
    pub(crate) fn eof(&mut self) -> Result<()> {
        Ok(())
    }
}
