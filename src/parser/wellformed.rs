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

/// Rejects any `name` that violates the XML 1.0 `Name` production (P5):
///
/// ```text
/// NameStartChar ::= Letter | '_' | ':'
/// NameChar      ::= Letter | Digit | '.' | '-' | '_' | ':' | CombiningChar | Extender
/// Name          ::= NameStartChar (NameChar)*
/// ```
///
/// The character classes (`Letter`, `Digit`, `CombiningChar`, `Extender`) are
/// the explicit code-point tables of XML 1.0 Appendix B. The full qualified
/// name is checked as a single `Name`: the `Name` production permits `:` in any
/// position, so prefixed names such as `ns:local` are accepted here and the
/// namespace-level `NCName` constraint is handled separately. `context` names
/// where the name came from, for the error message.
///
/// Every `NameChar` is also a legal `Char`, so a name accepted by this function
/// trivially satisfies the `Char` production; callers need not also call
/// [`check_chars`] on the same name.
pub(crate) fn check_name(name: &str, context: &str) -> Result<(), ParseError> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if is_name_start_char(c) => {}
        Some(c) => {
            return Err(ParseError::NotWellFormed {
                message: format!(
                    "illegal {context} start character U+{:04X} in name '{name}'",
                    c as u32
                ),
            });
        }
        None => {
            return Err(ParseError::NotWellFormed {
                message: format!("empty {context}"),
            });
        }
    }
    for c in chars {
        if !is_name_char(c) {
            return Err(ParseError::NotWellFormed {
                message: format!(
                    "illegal {context} character U+{:04X} in name '{name}'",
                    c as u32
                ),
            });
        }
    }
    Ok(())
}

/// True when `c` may begin a `Name` (`NameStartChar`): `Letter | '_' | ':'`.
#[inline]
pub(crate) fn is_name_start_char(c: char) -> bool {
    c == ':' || c == '_' || is_letter(c)
}

/// True when `c` may appear after the first character of a `Name` (`NameChar`):
/// `Letter | Digit | '.' | '-' | '_' | ':' | CombiningChar | Extender`.
#[inline]
pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || c == '-'
        || c == '.'
        || is_digit(c)
        || is_combining_char(c)
        || is_extender(c)
}

/// `Letter ::= BaseChar | Ideographic` (XML 1.0 P84).
#[inline]
fn is_letter(c: char) -> bool {
    is_base_char(c) || is_ideographic(c)
}

/// `BaseChar` (XML 1.0 P85).
fn is_base_char(c: char) -> bool {
    matches!(c,
        '\u{0041}'..='\u{005A}' | '\u{0061}'..='\u{007A}' | '\u{00C0}'..='\u{00D6}'
        | '\u{00D8}'..='\u{00F6}' | '\u{00F8}'..='\u{00FF}' | '\u{0100}'..='\u{0131}'
        | '\u{0134}'..='\u{013E}' | '\u{0141}'..='\u{0148}' | '\u{014A}'..='\u{017E}'
        | '\u{0180}'..='\u{01C3}' | '\u{01CD}'..='\u{01F0}' | '\u{01F4}'..='\u{01F5}'
        | '\u{01FA}'..='\u{0217}' | '\u{0250}'..='\u{02A8}' | '\u{02BB}'..='\u{02C1}'
        | '\u{0386}' | '\u{0388}'..='\u{038A}' | '\u{038C}' | '\u{038E}'..='\u{03A1}'
        | '\u{03A3}'..='\u{03CE}' | '\u{03D0}'..='\u{03D6}' | '\u{03DA}' | '\u{03DC}'
        | '\u{03DE}' | '\u{03E0}' | '\u{03E2}'..='\u{03F3}' | '\u{0401}'..='\u{040C}'
        | '\u{040E}'..='\u{044F}' | '\u{0451}'..='\u{045C}' | '\u{045E}'..='\u{0481}'
        | '\u{0490}'..='\u{04C4}' | '\u{04C7}'..='\u{04C8}' | '\u{04CB}'..='\u{04CC}'
        | '\u{04D0}'..='\u{04EB}' | '\u{04EE}'..='\u{04F5}' | '\u{04F8}'..='\u{04F9}'
        | '\u{0531}'..='\u{0556}' | '\u{0559}' | '\u{0561}'..='\u{0586}'
        | '\u{05D0}'..='\u{05EA}' | '\u{05F0}'..='\u{05F2}' | '\u{0621}'..='\u{063A}'
        | '\u{0641}'..='\u{064A}' | '\u{0671}'..='\u{06B7}' | '\u{06BA}'..='\u{06BE}'
        | '\u{06C0}'..='\u{06CE}' | '\u{06D0}'..='\u{06D3}' | '\u{06D5}'
        | '\u{06E5}'..='\u{06E6}' | '\u{0905}'..='\u{0939}' | '\u{093D}'
        | '\u{0958}'..='\u{0961}' | '\u{0985}'..='\u{098C}' | '\u{098F}'..='\u{0990}'
        | '\u{0993}'..='\u{09A8}' | '\u{09AA}'..='\u{09B0}' | '\u{09B2}'
        | '\u{09B6}'..='\u{09B9}' | '\u{09DC}'..='\u{09DD}' | '\u{09DF}'..='\u{09E1}'
        | '\u{09F0}'..='\u{09F1}' | '\u{0A05}'..='\u{0A0A}' | '\u{0A0F}'..='\u{0A10}'
        | '\u{0A13}'..='\u{0A28}' | '\u{0A2A}'..='\u{0A30}' | '\u{0A32}'..='\u{0A33}'
        | '\u{0A35}'..='\u{0A36}' | '\u{0A38}'..='\u{0A39}' | '\u{0A59}'..='\u{0A5C}'
        | '\u{0A5E}' | '\u{0A72}'..='\u{0A74}' | '\u{0A85}'..='\u{0A8B}' | '\u{0A8D}'
        | '\u{0A8F}'..='\u{0A91}' | '\u{0A93}'..='\u{0AA8}' | '\u{0AAA}'..='\u{0AB0}'
        | '\u{0AB2}'..='\u{0AB3}' | '\u{0AB5}'..='\u{0AB9}' | '\u{0ABD}' | '\u{0AE0}'
        | '\u{0B05}'..='\u{0B0C}' | '\u{0B0F}'..='\u{0B10}' | '\u{0B13}'..='\u{0B28}'
        | '\u{0B2A}'..='\u{0B30}' | '\u{0B32}'..='\u{0B33}' | '\u{0B36}'..='\u{0B39}'
        | '\u{0B3D}' | '\u{0B5C}'..='\u{0B5D}' | '\u{0B5F}'..='\u{0B61}'
        | '\u{0B85}'..='\u{0B8A}' | '\u{0B8E}'..='\u{0B90}' | '\u{0B92}'..='\u{0B95}'
        | '\u{0B99}'..='\u{0B9A}' | '\u{0B9C}' | '\u{0B9E}'..='\u{0B9F}'
        | '\u{0BA3}'..='\u{0BA4}' | '\u{0BA8}'..='\u{0BAA}' | '\u{0BAE}'..='\u{0BB5}'
        | '\u{0BB7}'..='\u{0BB9}' | '\u{0C05}'..='\u{0C0C}' | '\u{0C0E}'..='\u{0C10}'
        | '\u{0C12}'..='\u{0C28}' | '\u{0C2A}'..='\u{0C33}' | '\u{0C35}'..='\u{0C39}'
        | '\u{0C60}'..='\u{0C61}' | '\u{0C85}'..='\u{0C8C}' | '\u{0C8E}'..='\u{0C90}'
        | '\u{0C92}'..='\u{0CA8}' | '\u{0CAA}'..='\u{0CB3}' | '\u{0CB5}'..='\u{0CB9}'
        | '\u{0CDE}' | '\u{0CE0}'..='\u{0CE1}' | '\u{0D05}'..='\u{0D0C}'
        | '\u{0D0E}'..='\u{0D10}' | '\u{0D12}'..='\u{0D28}' | '\u{0D2A}'..='\u{0D39}'
        | '\u{0D60}'..='\u{0D61}' | '\u{0E01}'..='\u{0E2E}' | '\u{0E30}'
        | '\u{0E32}'..='\u{0E33}' | '\u{0E40}'..='\u{0E45}' | '\u{0E81}'..='\u{0E82}'
        | '\u{0E84}' | '\u{0E87}'..='\u{0E88}' | '\u{0E8A}' | '\u{0E8D}'
        | '\u{0E94}'..='\u{0E97}' | '\u{0E99}'..='\u{0E9F}' | '\u{0EA1}'..='\u{0EA3}'
        | '\u{0EA5}' | '\u{0EA7}' | '\u{0EAA}'..='\u{0EAB}' | '\u{0EAD}'..='\u{0EAE}'
        | '\u{0EB0}' | '\u{0EB2}'..='\u{0EB3}' | '\u{0EBD}' | '\u{0EC0}'..='\u{0EC4}'
        | '\u{0F40}'..='\u{0F47}' | '\u{0F49}'..='\u{0F69}' | '\u{10A0}'..='\u{10C5}'
        | '\u{10D0}'..='\u{10F6}' | '\u{1100}' | '\u{1102}'..='\u{1103}'
        | '\u{1105}'..='\u{1107}' | '\u{1109}' | '\u{110B}'..='\u{110C}'
        | '\u{110E}'..='\u{1112}' | '\u{113C}' | '\u{113E}' | '\u{1140}' | '\u{114C}'
        | '\u{114E}' | '\u{1150}' | '\u{1154}'..='\u{1155}' | '\u{1159}'
        | '\u{115F}'..='\u{1161}' | '\u{1163}' | '\u{1165}' | '\u{1167}' | '\u{1169}'
        | '\u{116D}'..='\u{116E}' | '\u{1172}'..='\u{1173}' | '\u{1175}' | '\u{119E}'
        | '\u{11A8}' | '\u{11AB}' | '\u{11AE}'..='\u{11AF}' | '\u{11B7}'..='\u{11B8}'
        | '\u{11BA}' | '\u{11BC}'..='\u{11C2}' | '\u{11EB}' | '\u{11F0}' | '\u{11F9}'
        | '\u{1E00}'..='\u{1E9B}' | '\u{1EA0}'..='\u{1EF9}' | '\u{1F00}'..='\u{1F15}'
        | '\u{1F18}'..='\u{1F1D}' | '\u{1F20}'..='\u{1F45}' | '\u{1F48}'..='\u{1F4D}'
        | '\u{1F50}'..='\u{1F57}' | '\u{1F59}' | '\u{1F5B}' | '\u{1F5D}'
        | '\u{1F5F}'..='\u{1F7D}' | '\u{1F80}'..='\u{1FB4}' | '\u{1FB6}'..='\u{1FBC}'
        | '\u{1FBE}' | '\u{1FC2}'..='\u{1FC4}' | '\u{1FC6}'..='\u{1FCC}'
        | '\u{1FD0}'..='\u{1FD3}' | '\u{1FD6}'..='\u{1FDB}' | '\u{1FE0}'..='\u{1FEC}'
        | '\u{1FF2}'..='\u{1FF4}' | '\u{1FF6}'..='\u{1FFC}' | '\u{2126}'
        | '\u{212A}'..='\u{212B}' | '\u{212E}' | '\u{2180}'..='\u{2182}'
        | '\u{3041}'..='\u{3094}' | '\u{30A1}'..='\u{30FA}' | '\u{3105}'..='\u{312C}'
        | '\u{AC00}'..='\u{D7A3}')
}

/// `Ideographic` (XML 1.0 P86).
#[inline]
fn is_ideographic(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FA5}' | '\u{3007}' | '\u{3021}'..='\u{3029}')
}

/// `CombiningChar` (XML 1.0 P87).
fn is_combining_char(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{0345}' | '\u{0360}'..='\u{0361}' | '\u{0483}'..='\u{0486}'
        | '\u{0591}'..='\u{05A1}' | '\u{05A3}'..='\u{05B9}' | '\u{05BB}'..='\u{05BD}'
        | '\u{05BF}' | '\u{05C1}'..='\u{05C2}' | '\u{05C4}' | '\u{064B}'..='\u{0652}'
        | '\u{0670}' | '\u{06D6}'..='\u{06DC}' | '\u{06DD}'..='\u{06DF}'
        | '\u{06E0}'..='\u{06E4}' | '\u{06E7}'..='\u{06E8}' | '\u{06EA}'..='\u{06ED}'
        | '\u{0901}'..='\u{0903}' | '\u{093C}' | '\u{093E}'..='\u{094C}' | '\u{094D}'
        | '\u{0951}'..='\u{0954}' | '\u{0962}'..='\u{0963}' | '\u{0981}'..='\u{0983}'
        | '\u{09BC}' | '\u{09BE}' | '\u{09BF}' | '\u{09C0}'..='\u{09C4}'
        | '\u{09C7}'..='\u{09C8}' | '\u{09CB}'..='\u{09CD}' | '\u{09D7}'
        | '\u{09E2}'..='\u{09E3}' | '\u{0A02}' | '\u{0A3C}' | '\u{0A3E}' | '\u{0A3F}'
        | '\u{0A40}'..='\u{0A42}' | '\u{0A47}'..='\u{0A48}' | '\u{0A4B}'..='\u{0A4D}'
        | '\u{0A70}'..='\u{0A71}' | '\u{0A81}'..='\u{0A83}' | '\u{0ABC}'
        | '\u{0ABE}'..='\u{0AC5}' | '\u{0AC7}'..='\u{0AC9}' | '\u{0ACB}'..='\u{0ACD}'
        | '\u{0B01}'..='\u{0B03}' | '\u{0B3C}' | '\u{0B3E}'..='\u{0B43}'
        | '\u{0B47}'..='\u{0B48}' | '\u{0B4B}'..='\u{0B4D}' | '\u{0B56}'..='\u{0B57}'
        | '\u{0B82}'..='\u{0B83}' | '\u{0BBE}'..='\u{0BC2}' | '\u{0BC6}'..='\u{0BC8}'
        | '\u{0BCA}'..='\u{0BCD}' | '\u{0BD7}' | '\u{0C01}'..='\u{0C03}'
        | '\u{0C3E}'..='\u{0C44}' | '\u{0C46}'..='\u{0C48}' | '\u{0C4A}'..='\u{0C4D}'
        | '\u{0C55}'..='\u{0C56}' | '\u{0C82}'..='\u{0C83}' | '\u{0CBE}'..='\u{0CC4}'
        | '\u{0CC6}'..='\u{0CC8}' | '\u{0CCA}'..='\u{0CCD}' | '\u{0CD5}'..='\u{0CD6}'
        | '\u{0D02}'..='\u{0D03}' | '\u{0D3E}'..='\u{0D43}' | '\u{0D46}'..='\u{0D48}'
        | '\u{0D4A}'..='\u{0D4D}' | '\u{0D57}' | '\u{0E31}' | '\u{0E34}'..='\u{0E3A}'
        | '\u{0E47}'..='\u{0E4E}' | '\u{0EB1}' | '\u{0EB4}'..='\u{0EB9}'
        | '\u{0EBB}'..='\u{0EBC}' | '\u{0EC8}'..='\u{0ECD}' | '\u{0F18}'..='\u{0F19}'
        | '\u{0F35}' | '\u{0F37}' | '\u{0F39}' | '\u{0F3E}' | '\u{0F3F}'
        | '\u{0F71}'..='\u{0F84}' | '\u{0F86}'..='\u{0F8B}' | '\u{0F90}'..='\u{0F95}'
        | '\u{0F97}' | '\u{0F99}'..='\u{0FAD}' | '\u{0FB1}'..='\u{0FB7}' | '\u{0FB9}'
        | '\u{20D0}'..='\u{20DC}' | '\u{20E1}' | '\u{302A}'..='\u{302F}' | '\u{3099}'
        | '\u{309A}')
}

/// `Digit` (XML 1.0 P88).
fn is_digit(c: char) -> bool {
    matches!(c,
        '\u{0030}'..='\u{0039}' | '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}'
        | '\u{0966}'..='\u{096F}' | '\u{09E6}'..='\u{09EF}' | '\u{0A66}'..='\u{0A6F}'
        | '\u{0AE6}'..='\u{0AEF}' | '\u{0B66}'..='\u{0B6F}' | '\u{0BE7}'..='\u{0BEF}'
        | '\u{0C66}'..='\u{0C6F}' | '\u{0CE6}'..='\u{0CEF}' | '\u{0D66}'..='\u{0D6F}'
        | '\u{0E50}'..='\u{0E59}' | '\u{0ED0}'..='\u{0ED9}' | '\u{0F20}'..='\u{0F29}')
}

/// `Extender` (XML 1.0 P89).
fn is_extender(c: char) -> bool {
    matches!(c,
        '\u{00B7}' | '\u{02D0}' | '\u{02D1}' | '\u{0387}' | '\u{0640}' | '\u{0E46}'
        | '\u{0EC6}' | '\u{3005}' | '\u{3031}'..='\u{3035}' | '\u{309D}'..='\u{309E}'
        | '\u{30FC}'..='\u{30FE}')
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

    #[test]
    fn name_start_chars() {
        // ASCII letters, '_' and ':' start names.
        assert!(is_name_start_char('a'));
        assert!(is_name_start_char('Z'));
        assert!(is_name_start_char('_'));
        assert!(is_name_start_char(':'));
        // Non-ASCII letters and ideographics.
        assert!(is_name_start_char('é')); // U+00E9 BaseChar
        assert!(is_name_start_char('日')); // U+65E5 Ideographic
        // Digits, punctuation, and combining marks do NOT start names.
        assert!(!is_name_start_char('0'));
        assert!(!is_name_start_char('-'));
        assert!(!is_name_start_char('.'));
        assert!(!is_name_start_char('\u{0300}')); // combining grave accent
    }

    #[test]
    fn name_chars_after_start() {
        assert!(is_name_char('a'));
        assert!(is_name_char('0'));
        assert!(is_name_char('-'));
        assert!(is_name_char('.'));
        assert!(is_name_char('\u{0300}')); // CombiningChar
        assert!(is_name_char('\u{00B7}')); // Extender (middle dot)
        assert!(!is_name_char(' '));
        assert!(!is_name_char('('));
    }

    #[test]
    fn base_char_gaps_are_rejected() {
        // Characters that sit in gaps of the BaseChar ranges must not be letters,
        // guarding against off-by-one transcription errors.
        assert!(!is_letter('\u{00D7}')); // × MULTIPLICATION SIGN (gap 00D6..00D8)
        assert!(!is_letter('\u{00F7}')); // ÷ DIVISION SIGN (gap 00F6..00F8)
        assert!(!is_letter('\u{0387}')); // GREEK ANO TELEIA (an Extender, not a letter)
    }

    #[test]
    fn check_name_accepts_valid_names() {
        assert!(check_name("root", "element name").is_ok());
        assert!(check_name("ns:local", "element name").is_ok()); // ':' allowed in Name
        assert!(check_name("_x.y-z0", "element name").is_ok());
        assert!(check_name("日本語", "element name").is_ok());
        assert!(check_name("café", "element name").is_ok());
    }

    #[test]
    fn check_name_rejects_invalid_names() {
        // Digit start.
        let err = check_name("1root", "element name").unwrap_err();
        assert!(err.to_string().contains("U+0031"));
        // Illegal interior character.
        assert!(check_name("a\u{00D7}b", "element name").is_err());
        // A space is never a NameChar.
        assert!(check_name("a b", "element name").is_err());
        // Empty name.
        assert!(check_name("", "element name").is_err());
    }
}
