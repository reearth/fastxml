//! Reference expansion for character data and attribute values, shared by the
//! DOM and streaming parsers.
//!
//! XML 1.0 §4.4 and Appendix D define what a reference means where it
//! appears:
//!
//! - In content, `&name;` is replaced by the entity's replacement text, which
//!   is then parsed as content: references inside it are expanded in turn,
//!   and markup inside it (`<b>x</b>`) produces elements, not text. Character
//!   data is returned as text; an entity whose replacement text contains
//!   markup is returned as a [`Segment::Markup`] for the caller to parse as a
//!   content fragment.
//! - In an attribute value (§3.3.3), references are expanded recursively,
//!   every literal white-space character (TAB, LF, CR) becomes a space,
//!   character references are kept as the character they denote, and a `<`
//!   arriving through an entity is a well-formedness error.
//!
//! Replacement texts come from [`super::entities::parse_internal_entities`],
//! which expands only character references at declaration time, as §4.5
//! requires.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::error::ParseError;
use super::wellformed::{check_name, is_xml_char};

/// Upper bound on the total size of entity replacement text expanded in one
/// document (16 MiB). Protects against exponential "billion laughs" entity
/// definitions, whose expansion would otherwise exhaust memory.
pub(crate) const MAX_ENTITY_EXPANSION: usize = 16 * 1024 * 1024;

/// Expands references against the general entities declared in a document's
/// internal DTD subset, tracking which entities are being expanded so a
/// recursive reference is rejected.
#[derive(Debug, Default)]
pub(crate) struct EntityExpander {
    /// Entity name to replacement text.
    replacements: HashMap<String, Arc<str>>,
    /// Declared external parsed entities. fastxml does not read external
    /// entities, so a reference to one is reported as an error rather than
    /// silently dropping its content.
    external: HashSet<String>,
    /// Entities currently being expanded, outermost first.
    open: Vec<String>,
    /// Total replacement-text bytes expanded so far.
    expanded: usize,
}

/// A piece of expanded character data.
#[derive(Debug, PartialEq)]
pub(crate) enum Segment {
    /// Character data.
    Text(String),
    /// A reference to the named entity, whose replacement text contains
    /// markup and must be parsed as content (see [`EntityExpander::enter`]).
    Markup(String),
}

/// The result of expanding the references in one run of character data.
#[derive(Debug)]
pub(crate) enum TextExpansion<'a> {
    /// Plain character data (borrowed when it held no reference).
    Text(Cow<'a, str>),
    /// Character data interleaved with entities that contain markup.
    Segments(Vec<Segment>),
}

/// One parsed reference.
enum Reference<'a> {
    /// A character reference or predefined entity.
    Char(char),
    /// A general entity reference.
    Entity(&'a str),
}

impl EntityExpander {
    /// Creates an expander with no declared entities.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Replaces the declared internal entities (name → replacement text).
    pub(crate) fn declare(&mut self, entities: HashMap<String, String>) {
        self.replacements = entities
            .into_iter()
            .map(|(name, text)| (name, Arc::from(text)))
            .collect();
    }

    /// Declares the general entities of the document's `DOCTYPE` once the
    /// well-formedness checker has seen the complete declaration.
    pub(crate) fn declare_from(&mut self, checker: &mut super::checks::WellformedChecker) {
        if let Some(doctype) = checker.take_doctype() {
            let decls = super::entities::parse_entity_declarations(&doctype);
            self.declare(decls.replacements);
            self.external = decls.external;
        }
    }

    /// Expands the references in raw character data from content.
    pub(crate) fn expand_text<'a>(
        &mut self,
        raw: &'a str,
    ) -> Result<TextExpansion<'a>, ParseError> {
        if memchr::memchr(b'&', raw.as_bytes()).is_none() {
            return Ok(TextExpansion::Text(Cow::Borrowed(raw)));
        }
        let mut acc = String::with_capacity(raw.len());
        let mut segments = Vec::new();
        self.text_into(raw, &mut acc, &mut segments)?;
        if segments.is_empty() {
            return Ok(TextExpansion::Text(Cow::Owned(acc)));
        }
        if !acc.is_empty() {
            segments.push(Segment::Text(acc));
        }
        Ok(TextExpansion::Segments(segments))
    }

    fn text_into(
        &mut self,
        s: &str,
        acc: &mut String,
        segments: &mut Vec<Segment>,
    ) -> Result<(), ParseError> {
        let mut rest = s;
        while let Some(amp) = memchr::memchr(b'&', rest.as_bytes()) {
            acc.push_str(&rest[..amp]);
            let (reference, after) = parse_reference(&rest[amp..], text_error)?;
            match reference {
                Reference::Char(c) => acc.push(c),
                Reference::Entity(name) if self.is_external(name) => {
                    return Err(text_error(format!(
                        "'&{name};' refers to an external entity, which fastxml does not load"
                    )));
                }
                Reference::Entity(name) => {
                    let replacement = self.lookup(name, text_error)?;
                    if replacement.contains('<') {
                        if !acc.is_empty() {
                            segments.push(Segment::Text(std::mem::take(acc)));
                        }
                        segments.push(Segment::Markup(name.to_string()));
                    } else {
                        self.push_open(name, &replacement)?;
                        let result = self.text_into(&replacement, acc, segments);
                        self.open.pop();
                        result?;
                    }
                }
            }
            rest = after;
        }
        acc.push_str(rest);
        Ok(())
    }

    /// Expands and normalizes a raw attribute value (XML 1.0 §3.3.3, CDATA
    /// attribute type).
    pub(crate) fn expand_attr<'a>(&mut self, raw: &'a str) -> Result<Cow<'a, str>, ParseError> {
        if !raw
            .bytes()
            .any(|b| matches!(b, b'&' | b'\t' | b'\n' | b'\r'))
        {
            return Ok(Cow::Borrowed(raw));
        }
        let mut out = String::with_capacity(raw.len());
        self.attr_into(raw, &mut out, false)?;
        Ok(Cow::Owned(out))
    }

    fn attr_into(&mut self, s: &str, out: &mut String, in_entity: bool) -> Result<(), ParseError> {
        let mut rest = s;
        loop {
            let Some(i) = rest
                .bytes()
                .position(|b| matches!(b, b'&' | b'\t' | b'\n' | b'\r' | b'<'))
            else {
                out.push_str(rest);
                return Ok(());
            };
            out.push_str(&rest[..i]);
            match rest.as_bytes()[i] {
                b'&' => {
                    let (reference, after) = parse_reference(&rest[i..], attr_error)?;
                    match reference {
                        Reference::Char(c) => out.push(c),
                        Reference::Entity(name) if self.is_external(name) => {
                            return Err(ParseError::NotWellFormed {
                                message: format!(
                                    "attribute values must not reference the external entity '{name}'"
                                ),
                            });
                        }
                        Reference::Entity(name) => {
                            let replacement = self.lookup(name, attr_error)?;
                            self.push_open(name, &replacement)?;
                            let result = self.attr_into(&replacement, out, true);
                            self.open.pop();
                            result?;
                        }
                    }
                    rest = after;
                }
                b'<' if in_entity => {
                    return Err(ParseError::NotWellFormed {
                        message: "'<' is not allowed in an attribute value, \
                                  including through an entity reference"
                            .to_string(),
                    });
                }
                b'<' => {
                    // The raw value was already checked for `<`; keep as-is.
                    out.push('<');
                    rest = &rest[i + 1..];
                }
                _ => {
                    out.push(' ');
                    rest = &rest[i + 1..];
                }
            }
        }
    }

    /// Opens the named entity for parsing as a content fragment and returns
    /// its replacement text. Call [`leave`](Self::leave) when the fragment is
    /// done.
    pub(crate) fn enter(&mut self, name: &str) -> Result<Arc<str>, ParseError> {
        let replacement = self.lookup(name, text_error)?;
        self.push_open(name, &replacement)?;
        Ok(replacement)
    }

    /// Closes the entity most recently opened by [`enter`](Self::enter).
    pub(crate) fn leave(&mut self) {
        self.open.pop();
    }

    /// Whether `name` is an external parsed entity (and not also internal).
    fn is_external(&self, name: &str) -> bool {
        !self.replacements.contains_key(name) && self.external.contains(name)
    }

    fn lookup(&self, name: &str, err: fn(String) -> ParseError) -> Result<Arc<str>, ParseError> {
        self.replacements
            .get(name)
            .cloned()
            .ok_or_else(|| err(format!("unknown entity reference '&{name};'")))
    }

    /// Records that `name` is being expanded, rejecting recursion and
    /// runaway expansion.
    fn push_open(&mut self, name: &str, replacement: &str) -> Result<(), ParseError> {
        if self.open.iter().any(|n| n == name) {
            return Err(ParseError::NotWellFormed {
                message: format!("entity '{name}' references itself"),
            });
        }
        self.expanded = self.expanded.saturating_add(replacement.len());
        if self.expanded > MAX_ENTITY_EXPANSION {
            return Err(ParseError::NotWellFormed {
                message: format!(
                    "entity expansion exceeds the limit of {MAX_ENTITY_EXPANSION} bytes"
                ),
            });
        }
        self.open.push(name.to_string());
        Ok(())
    }
}

/// A tokenizer over an entity's replacement text, configured like the
/// document readers (end tags checked against start tags, empty-element tags
/// reported as a start and an end).
pub(crate) fn fragment_reader(text: &str) -> quick_xml::Reader<&[u8]> {
    let mut reader = quick_xml::Reader::from_str(text);
    let config = reader.config_mut();
    config.trim_text(false);
    config.expand_empty_elements = true;
    config.check_end_names = true;
    config.check_comments = true;
    reader
}

/// Error for a tokenizer failure inside the replacement text of `name`.
pub(crate) fn fragment_error(name: &str, e: quick_xml::Error) -> ParseError {
    ParseError::NotWellFormed {
        message: format!("in the replacement text of entity '{name}': {e}"),
    }
}

/// The replacement text of an entity used in content must itself be
/// balanced content (XML 1.0 §4.3.2): every element it opens closes in it.
pub(crate) fn check_balanced(
    name: &str,
    depth_before: usize,
    depth_after: usize,
) -> Result<(), ParseError> {
    if depth_before == depth_after {
        Ok(())
    } else {
        Err(ParseError::NotWellFormed {
            message: format!(
                "the replacement text of entity '{name}' does not contain balanced elements"
            ),
        })
    }
}

fn text_error(message: String) -> ParseError {
    ParseError::TextDecodeError { message }
}

fn attr_error(message: String) -> ParseError {
    ParseError::AttributeDecodeError { message }
}

/// Parses the reference at the start of `s` (which begins with `&`),
/// returning it and the text after its `;`.
fn parse_reference(
    s: &str,
    err: fn(String) -> ParseError,
) -> Result<(Reference<'_>, &str), ParseError> {
    let Some(semi) = memchr::memchr(b';', s.as_bytes()) else {
        return Err(err(
            "'&' must start a reference terminated by ';'".to_string()
        ));
    };
    let body = &s[1..semi];
    let after = &s[semi + 1..];
    if let Some(num) = body.strip_prefix('#') {
        // CharRef ::= '&#' [0-9]+ ';' | '&#x' [0-9a-fA-F]+ ';' (lowercase x only)
        let code = match num.strip_prefix('x') {
            Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u32::from_str_radix(hex, 16).ok()
            }
            None if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) => {
                num.parse::<u32>().ok()
            }
            _ => {
                return Err(err(format!("malformed character reference '&{body};'")));
            }
        };
        return match code.and_then(char::from_u32).filter(|&c| is_xml_char(c)) {
            Some(c) => Ok((Reference::Char(c), after)),
            None => Err(ParseError::NotWellFormed {
                message: format!("character reference '&{body};' denotes an illegal character"),
            }),
        };
    }
    let reference = match body {
        "lt" => Reference::Char('<'),
        "gt" => Reference::Char('>'),
        "amp" => Reference::Char('&'),
        "quot" => Reference::Char('"'),
        "apos" => Reference::Char('\''),
        name => {
            check_name(name, "entity reference")?;
            Reference::Entity(name)
        }
    };
    Ok((reference, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expander(decls: &[(&str, &str)]) -> EntityExpander {
        let mut e = EntityExpander::new();
        e.declare(
            decls
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
        );
        e
    }

    fn text(e: &mut EntityExpander, raw: &str) -> String {
        match e.expand_text(raw).unwrap() {
            TextExpansion::Text(t) => t.into_owned(),
            other => panic!("expected plain text, got {other:?}"),
        }
    }

    #[test]
    fn plain_text_is_borrowed() {
        let mut e = expander(&[]);
        assert!(matches!(
            e.expand_text("abc").unwrap(),
            TextExpansion::Text(Cow::Borrowed("abc"))
        ));
        assert!(matches!(
            e.expand_attr("abc").unwrap(),
            Cow::Borrowed("abc")
        ));
    }

    #[test]
    fn replacement_text_is_rescanned() {
        let mut e = expander(&[("a", "x&amp;y"), ("b", "&a;&lt;")]);
        assert_eq!(text(&mut e, "1&b;2"), "1x&y<2");
        assert_eq!(e.expand_attr("1&b;2").unwrap(), "1x&y<2");
    }

    #[test]
    fn markup_entities_become_segments() {
        let mut e = expander(&[("m", "<i/>"), ("t", "x&m;y")]);
        match e.expand_text("a&t;b").unwrap() {
            TextExpansion::Segments(s) => assert_eq!(
                s,
                [
                    Segment::Text("ax".into()),
                    Segment::Markup("m".into()),
                    Segment::Text("yb".into()),
                ]
            ),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn attribute_whitespace_is_normalized_but_char_refs_are_kept() {
        let mut e = expander(&[("w", "a\tb")]);
        assert_eq!(
            e.expand_attr("x\ty\nz&#9;&#10;&#13;").unwrap(),
            "x y z\t\n\r"
        );
        assert_eq!(e.expand_attr("&w;").unwrap(), "a b");
    }

    #[test]
    fn lt_through_entity_in_attribute_is_rejected() {
        let mut e = expander(&[("l", "<")]);
        assert!(e.expand_attr("&l;").is_err());
        let mut e = expander(&[("l", "&#60;")]);
        assert_eq!(e.expand_attr("&l;").unwrap(), "<");
    }

    #[test]
    fn errors() {
        let mut e = expander(&[("a", "&b;"), ("b", "&a;")]);
        assert!(e.expand_text("&a;").is_err(), "recursion");
        assert!(e.expand_text("&nope;").is_err(), "undeclared");
        assert!(e.expand_text("&#X41;").is_err(), "uppercase X");
        assert!(e.expand_text("&#1;").is_err(), "illegal char");
        assert!(e.expand_text("a & b").is_err(), "bare ampersand");
    }
}
