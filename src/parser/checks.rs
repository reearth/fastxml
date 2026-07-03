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

/// Where in the document the parser currently is, per the top-level grammar
/// `document ::= prolog element Misc*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocState {
    /// Before the document (root) element: the `prolog`.
    Prolog,
    /// Inside the document element, at the given nesting depth (≥ 1).
    InRoot(usize),
    /// After the document element has closed: trailing `Misc*`.
    Epilog,
}

/// Owns the per-event well-formedness checks for one parse.
///
/// Construct one per document and call the matching method for each event the
/// tokenizer produces. Methods borrow `&mut self` because several
/// well-formedness rules accumulate state across events (document structure,
/// and later namespace scopes).
#[derive(Debug)]
pub(crate) struct WellformedChecker {
    /// Position in the top-level document grammar.
    state: DocState,
    /// Whether a document (root) element has been seen at all.
    root_seen: bool,
    /// Whether a `DOCTYPE` declaration has already appeared.
    doctype_seen: bool,
    /// Set while the `DOCTYPE` internal subset is still open because quick-xml
    /// truncated the `DocType` event at a `>` inside a quoted literal. The
    /// following text events carry the rest of the subset until its closing
    /// `]`; they must not be judged as prolog character data.
    dtd_open: bool,
}

impl Default for WellformedChecker {
    fn default() -> Self {
        Self {
            state: DocState::Prolog,
            root_seen: false,
            doctype_seen: false,
            dtd_open: false,
        }
    }
}

/// Scans an internal-subset fragment (the text *after* the opening `[`) for the
/// `]` that closes the subset at top level, honoring quoted literals and
/// comments. Returns the byte offset of that `]`, or `None` if the fragment
/// does not close the subset (quick-xml truncated the declaration).
fn internal_subset_end(fragment: &str) -> Option<usize> {
    let bytes = fragment.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b']' => return Some(i),
            b'<' if bytes[i..].starts_with(b"<!--") => {
                // Skip a comment; interior `]`/`"` are not significant.
                if let Some(rel) = fragment[i + 4..].find("-->") {
                    i += 4 + rel + 3;
                    continue;
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// True for the four characters of the XML `S` (white space) production.
#[inline]
fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn not_wf(message: impl Into<String>) -> ParseError {
    ParseError::NotWellFormed {
        message: message.into(),
    }
}

impl WellformedChecker {
    /// Creates a checker for a fresh document.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Checks a start (or empty-element) tag: the element `Name`, every
    /// attribute's `Name` and raw value, and the document-structure position.
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
            let value = std::str::from_utf8(attr.value.as_ref())?;
            check_chars(value, "attribute value")?;
            // `<` is forbidden in an attribute value (the AttValue production
            // excludes it); a literal here can never be a character reference.
            if value.contains('<') {
                return Err(not_wf(format!(
                    "'<' is not allowed in the value of attribute '{key}'"
                ))
                .into());
            }
        }
        self.open_element()?;
        Ok(())
    }

    /// Structural transition when an element opens.
    fn open_element(&mut self) -> Result<()> {
        match self.state {
            DocState::Prolog => {
                self.state = DocState::InRoot(1);
                self.root_seen = true;
            }
            DocState::InRoot(depth) => self.state = DocState::InRoot(depth + 1),
            DocState::Epilog => {
                return Err(not_wf(
                    "only one document element is allowed; found content after the root element",
                )
                .into());
            }
        }
        Ok(())
    }

    /// Checks an end tag's `Name` and closes an element structurally.
    ///
    /// With `check_end_names` enabled (the default for both engines) the
    /// tokenizer guarantees this name equals the matching start tag's name,
    /// which [`start`](Self::start) already validated, so the name check is
    /// effectively a no-op that keeps the two loops symmetric.
    pub(crate) fn end(&mut self, qname: &str) -> Result<()> {
        check_name(qname, "element name")?;
        if let DocState::InRoot(depth) = self.state {
            self.state = if depth <= 1 {
                DocState::Epilog
            } else {
                DocState::InRoot(depth - 1)
            };
        }
        Ok(())
    }

    /// Checks raw text (character data).
    pub(crate) fn text(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "text content")?;
        // When quick-xml truncated the DOCTYPE at a `>` inside a quoted literal,
        // the remaining internal subset (up to its closing `]`) arrives as text.
        // Consume it as DTD rather than judging it as prolog character data.
        if self.dtd_open {
            if let Some(end) = internal_subset_end(raw) {
                self.dtd_open = false;
                // Anything after the `]` and the DOCTYPE's `>` is prolog text.
                let tail = &raw[end + 1..];
                let after = tail.strip_prefix('>').unwrap_or(tail);
                return self.text(after);
            }
            return Ok(());
        }
        // `]]>` may not appear literally in character data (CDATA-section-close
        // delimiter); it must be written `]]&gt;`.
        if raw.contains("]]>") {
            return Err(not_wf("the sequence ']]>' is not allowed in character data").into());
        }
        // Outside the root element only white space is permitted as text.
        if matches!(self.state, DocState::Prolog | DocState::Epilog)
            && !raw.chars().all(is_xml_space)
        {
            return Err(
                not_wf("character data is only allowed inside the document element").into(),
            );
        }
        Ok(())
    }

    /// Checks a CDATA section body.
    pub(crate) fn cdata(&mut self, raw: &str) -> Result<()> {
        check_chars(raw, "CDATA section")?;
        if !matches!(self.state, DocState::InRoot(_)) {
            return Err(
                not_wf("a CDATA section is only allowed inside the document element").into(),
            );
        }
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
        // The target is the leading Name; data (if any) follows white space.
        let target = raw.split(is_xml_space).next().unwrap_or("");
        if target.is_empty() {
            return Err(not_wf("a processing instruction must have a target").into());
        }
        check_name(target, "processing-instruction target")?;
        // `xml` (in any case) is a reserved PITarget.
        if target.eq_ignore_ascii_case("xml") {
            return Err(not_wf(format!(
                "'{target}' is a reserved processing-instruction target"
            ))
            .into());
        }
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
        if self.doctype_seen {
            return Err(not_wf("only one DOCTYPE declaration is allowed").into());
        }
        if self.state != DocState::Prolog {
            return Err(
                not_wf("the DOCTYPE declaration must appear before the document element").into(),
            );
        }
        self.doctype_seen = true;
        // If an internal subset opens (`[`) but its closing `]` is absent, the
        // tokenizer truncated the declaration at a `>` inside a quoted literal;
        // the remainder follows as text events (handled in `text`). When it is
        // complete, validate the whole declaration's grammar.
        match raw.split_once('[').map(|(_, rest)| rest) {
            Some(after_bracket) if internal_subset_end(after_bracket).is_none() => {
                self.dtd_open = true;
            }
            _ => super::dtd::check_doctype(raw)?,
        }
        Ok(())
    }

    /// Called once at end of input.
    pub(crate) fn eof(&mut self) -> Result<()> {
        if !self.root_seen {
            return Err(not_wf("no document element found").into());
        }
        if matches!(self.state, DocState::InRoot(_)) {
            return Err(not_wf("unexpected end of input: an element was left unclosed").into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_subset_end_honors_quotes_and_comments() {
        // The first top-level `]` closes the subset.
        assert_eq!(internal_subset_end(" x ]>"), Some(3));
        // `]` inside a quoted literal is not the end.
        assert_eq!(internal_subset_end("<!ENTITY e \"a]b\">]"), Some(17));
        // `]` inside a comment is not the end.
        assert_eq!(internal_subset_end("<!-- ] -->]"), Some(10));
        // A truncated subset never closes.
        assert_eq!(internal_subset_end("<!ENTITY gt \">\""), None);
    }

    #[test]
    fn is_space_matches_only_xml_s() {
        for c in [' ', '\t', '\r', '\n'] {
            assert!(is_xml_space(c));
        }
        for c in ['\u{0B}', '\u{0C}', 'a', '\u{A0}'] {
            assert!(!is_xml_space(c));
        }
    }
}
