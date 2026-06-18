//! XML well-formedness character checks shared by the DOM and streaming
//! parsers.
//!
//! The underlying tokenizer (quick-xml) is lenient about character legality,
//! so it admits documents the XML specification requires to be rejected. This
//! module enforces the `Char` production, which is identical across XML 1.0
//! and 1.1 for *literal* characters:
//!
//! ```text
//! Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]
//! ```
//!
//! Anything outside that set — C0 controls other than tab/LF/CR, the surrogate
//! block, and the non-characters `#xFFFE`/`#xFFFF` — makes the document
//! not well-formed wherever it appears (names, text, attribute values, the
//! internal DTD subset, …). A conforming document never contains such a
//! codepoint, so this check cannot reject valid input.

use super::error::ParseError;

/// True when `c` is a legal XML character (the `Char` production).
#[inline]
pub(crate) fn is_xml_char(c: char) -> bool {
    matches!(c,
        '\u{9}' | '\u{A}' | '\u{D}'
        | '\u{20}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}')
}

/// Rejects any character in `s` that violates the `Char` production. `context`
/// names where the text came from, for the error message.
pub(crate) fn check_chars(s: &str, context: &str) -> Result<(), ParseError> {
    for c in s.chars() {
        if !is_xml_char(c) {
            return Err(ParseError::NotWellFormed {
                message: format!("illegal XML character U+{:04X} in {context}", c as u32),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_chars() {
        assert!(is_xml_char('\t'));
        assert!(is_xml_char('\n'));
        assert!(is_xml_char('\r'));
        assert!(is_xml_char(' '));
        assert!(is_xml_char('a'));
        assert!(is_xml_char('日'));
        assert!(is_xml_char('\u{1F600}'));
        assert!(is_xml_char('\u{FFFD}'));
    }

    #[test]
    fn illegal_chars() {
        assert!(!is_xml_char('\u{0}'));
        assert!(!is_xml_char('\u{B}')); // vertical tab
        assert!(!is_xml_char('\u{C}')); // form feed
        assert!(!is_xml_char('\u{1F}'));
        assert!(!is_xml_char('\u{FFFE}'));
        assert!(!is_xml_char('\u{FFFF}'));
    }

    #[test]
    fn check_reports_first_violation() {
        assert!(check_chars("hello", "text").is_ok());
        let err = check_chars("a\u{0C}b", "text").unwrap_err();
        assert!(err.to_string().contains("U+000C"));
    }
}
