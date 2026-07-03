//! Restricted-XPath grammar validation for identity-constraint selector and
//! field expressions (XSD 1.0 §3.11.6).
//!
//! The grammar admitted for `xs:selector`:
//!
//! ```text
//! Selector ::= Path ( '|' Path )*
//! Path     ::= ('.//')? Step ( '/' Step )*
//! Step     ::= '.' | ('child::')? NameTest
//! NameTest ::= QName | '*' | NCName ':' '*'
//! ```
//!
//! and for `xs:field` the same, except the last step may be an attribute test
//! (`@NameTest` / `attribute::NameTest`). Whitespace is allowed *between*
//! XPath tokens (`. //.`, `child ::a`) but not inside a QName or between the
//! two colons of `::` / the two slashes of `//`, matching XPath tokenization.
//!
//! This is a *validator* of the grammar; the streaming identity matcher
//! (`validator::streaming::identity`) stays the lenient runtime interpreter.

use crate::schema::xsd::primitive::PrimitiveKind;

/// Which identity-constraint expression is being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityXPathKind {
    /// `xs:selector/@xpath` — element paths only.
    Selector,
    /// `xs:field/@xpath` — may end in an attribute test.
    Field,
}

/// A name test token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NameTest {
    /// `*`
    Star,
    /// `prefix:*`
    PrefixStar(String),
    /// `local`
    Local(String),
    /// `prefix:local` (carries the prefix)
    Prefixed(String),
}

/// One XPath token of the restricted subset.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Dot,
    Slash,
    DoubleSlash,
    ColonColon,
    At,
    Name(NameTest),
}

/// Validates an identity-constraint XPath against the restricted grammar.
/// `prefix_declared` reports whether a namespace prefix is in scope.
pub(crate) fn validate_identity_xpath(
    xpath: &str,
    kind: IdentityXPathKind,
    prefix_declared: impl Fn(&str) -> bool,
) -> Result<(), String> {
    if xpath.trim().is_empty() {
        return Err("xpath must not be empty".to_string());
    }
    for alternative in xpath.split('|') {
        let tokens = tokenize(alternative)?;
        validate_path(&tokens, kind, &prefix_declared)
            .map_err(|m| format!("{m} in '{}'", alternative.trim()))?;
    }
    Ok(())
}

/// Whether `c` can start an NCName. Non-ASCII characters are letters as far
/// as this tokenizer is concerned; `PrimitiveKind::Ncname` does the precise
/// per-character validation afterwards.
fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || !c.is_ascii()
}

/// Whether `c` can continue an NCName.
fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.'
}

/// Tokenizes one path alternative. Whitespace separates tokens; QNames and
/// the `::` / `//` pairs must be contiguous.
fn tokenize(path: &str) -> Result<Vec<Tok>, String> {
    let mut tokens = Vec::new();
    let mut chars = path.char_indices().peekable();
    let bytes = path;

    while let Some((i, c)) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '/' => {
                if matches!(chars.peek(), Some((_, '/'))) {
                    chars.next();
                    tokens.push(Tok::DoubleSlash);
                } else {
                    tokens.push(Tok::Slash);
                }
            }
            ':' => {
                if matches!(chars.peek(), Some((_, ':'))) {
                    chars.next();
                    tokens.push(Tok::ColonColon);
                } else {
                    return Err(format!("unexpected ':' in '{}'", path.trim()));
                }
            }
            '@' => tokens.push(Tok::At),
            '*' => tokens.push(Tok::Name(NameTest::Star)),
            '.' => {
                // NCNames cannot start with '.', so a leading '.' is the
                // self step ('..' is rejected by the grammar below).
                tokens.push(Tok::Dot);
            }
            c if is_name_start(c) => {
                let start = i;
                let mut end = i + c.len_utf8();
                while let Some(&(j, cc)) = chars.peek() {
                    if is_name_char(cc) {
                        end = j + cc.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let name = &bytes[start..end];
                if PrimitiveKind::Ncname.validate(name).is_err() {
                    return Err(format!("'{name}' is not a valid NCName"));
                }
                // A ':' immediately after the NCName (not '::') binds it into
                // a QName / prefix wildcard; it must be contiguous.
                let is_qname_colon = matches!(chars.peek(), Some((j, ':')) if *j == end)
                    && !bytes[end + 1..].starts_with(':');
                if is_qname_colon {
                    chars.next(); // consume ':'
                    match chars.peek().copied() {
                        Some((j, '*')) if j == end + 1 => {
                            chars.next();
                            tokens.push(Tok::Name(NameTest::PrefixStar(name.to_string())));
                        }
                        Some((j, cc)) if j == end + 1 && is_name_start(cc) => {
                            let lstart = j;
                            let mut lend = j + cc.len_utf8();
                            chars.next();
                            while let Some(&(k, c2)) = chars.peek() {
                                if is_name_char(c2) {
                                    lend = k + c2.len_utf8();
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            let local = &bytes[lstart..lend];
                            if PrimitiveKind::Ncname.validate(local).is_err() {
                                return Err(format!("'{local}' is not a valid NCName"));
                            }
                            tokens.push(Tok::Name(NameTest::Prefixed(name.to_string())));
                        }
                        _ => {
                            return Err(format!("dangling ':' after '{name}'"));
                        }
                    }
                } else {
                    tokens.push(Tok::Name(NameTest::Local(name.to_string())));
                }
            }
            _ => {
                return Err(format!("unexpected character '{c}'"));
            }
        }
    }
    Ok(tokens)
}

/// Extracts the namespace prefix a name test carries, if any.
fn name_test_prefix(test: &NameTest) -> Option<&str> {
    match test {
        NameTest::PrefixStar(p) | NameTest::Prefixed(p) => Some(p.as_str()),
        NameTest::Star | NameTest::Local(_) => None,
    }
}

/// Validates one tokenized `|`-alternative against the step grammar.
fn validate_path(
    tokens: &[Tok],
    kind: IdentityXPathKind,
    prefix_declared: &impl Fn(&str) -> bool,
) -> Result<(), String> {
    if tokens.is_empty() {
        return Err("empty path alternative".to_string());
    }

    let check_prefix = |test: &NameTest| -> Result<(), String> {
        if let Some(prefix) = name_test_prefix(test)
            && prefix != "xml"
            && !prefix_declared(prefix)
        {
            return Err(format!("undeclared namespace prefix '{prefix}'"));
        }
        Ok(())
    };

    let mut i = 0;
    // Optional leading `.//`.
    if tokens.len() >= 2 && tokens[0] == Tok::Dot && tokens[1] == Tok::DoubleSlash {
        i = 2;
        if i == tokens.len() {
            return Err("'.//' must be followed by a step".to_string());
        }
    }

    loop {
        // One step.
        match &tokens[i..] {
            // Attribute test: `@NameTest` — final step of a field only.
            [Tok::At, Tok::Name(test)] => {
                if kind != IdentityXPathKind::Field {
                    return Err("attribute test is not allowed in a selector".to_string());
                }
                check_prefix(test)?;
                return Ok(());
            }
            // Axis step: `name::NameTest`. Only child:: anywhere, and
            // attribute:: as the final step of a field.
            [Tok::Name(axis), Tok::ColonColon, Tok::Name(test), rest @ ..] => {
                let is_last = rest.is_empty();
                let axis_name = match axis {
                    NameTest::Local(name) => name.as_str(),
                    _ => return Err("invalid axis".to_string()),
                };
                match axis_name {
                    "child" => {}
                    "attribute" if kind == IdentityXPathKind::Field && is_last => {
                        check_prefix(test)?;
                        return Ok(());
                    }
                    _ => return Err(format!("axis '{axis_name}::' is not allowed")),
                }
                check_prefix(test)?;
                if is_last {
                    return Ok(());
                }
                i += 3;
            }
            [Tok::Dot, rest @ ..] => {
                if rest.is_empty() {
                    return Ok(());
                }
                i += 1;
            }
            [Tok::Name(test), rest @ ..] => {
                check_prefix(test)?;
                if rest.is_empty() {
                    return Ok(());
                }
                i += 1;
            }
            _ => return Err("invalid step".to_string()),
        }
        // Between steps: exactly one '/'.
        match tokens.get(i) {
            Some(Tok::Slash) => {
                i += 1;
                if i == tokens.len() {
                    return Err("trailing '/'".to_string());
                }
            }
            _ => return Err("invalid step separator".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(xpath: &str) -> Result<(), String> {
        validate_identity_xpath(xpath, IdentityXPathKind::Selector, |p| p == "tn")
    }

    fn field(xpath: &str) -> Result<(), String> {
        validate_identity_xpath(xpath, IdentityXPathKind::Field, |p| p == "tn")
    }

    #[test]
    fn valid_selectors() {
        for x in [
            ".",
            "a",
            "a/b",
            ".//a",
            "./a",
            "tn:a/tn:b",
            "*",
            "tn:*",
            "a | b",
            ".//*",
            "child::a",
            ".//a/./b",
            ". //.",
            "child ::a",
            "child:: a",
            "child :: a",
            "a / b",
        ] {
            assert!(selector(x).is_ok(), "selector '{x}' should be valid");
        }
    }

    #[test]
    fn invalid_selectors() {
        for x in [
            "",
            " ",
            "/",
            "//",
            "|",
            "| a",
            "a |",
            "child::",
            ".//",
            "./ /.",
            ".//.//a",
            "a/.//b",
            "/a",
            "a//b",
            "a/",
            "@a",
            "attribute::a",
            "self::*",
            "descendant::*",
            "descendant-or-self::*",
            "a[b]",
            "a[@x='1']",
            "..",
            "a/..",
            "ncname :*",
            "ncname: *",
            "ncname : *",
            "child: :a",
            "undeclared:a",
            "count(a)",
            "@*",
        ] {
            assert!(selector(x).is_err(), "selector '{x}' should be invalid");
        }
    }

    #[test]
    fn valid_fields() {
        for x in [
            ".",
            "a",
            "a/b",
            "@a",
            "a/@b",
            ".//a/@b",
            "attribute::a",
            "tn:a/@tn:b",
            "@*",
            "@ a",
        ] {
            assert!(field(x).is_ok(), "field '{x}' should be valid");
        }
    }

    #[test]
    fn invalid_fields() {
        for x in [
            "",
            "@",
            "attribute::",
            "@a/b",
            "a/@b/c",
            "/",
            "//",
            "a//@b",
            "self::a",
        ] {
            assert!(field(x).is_err(), "field '{x}' should be invalid");
        }
    }
}
