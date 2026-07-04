//! Strict well-formedness checking of the `DOCTYPE` declaration and its
//! internal subset.
//!
//! quick-xml hands the whole `DOCTYPE` — root name, external id, and internal
//! subset — over as one raw `DocType` event and does not validate its grammar.
//! [`check_doctype`] performs a recursive-descent pass over that raw text to
//! reject the many not-well-formed declarations the tokenizer would otherwise
//! accept: bad processing-instruction targets inside the subset, malformed
//! `ELEMENT`/`ATTLIST`/`ENTITY`/`NOTATION` declarations, conditional sections
//! (illegal in the internal subset), illegal `PubidLiteral` characters, and so
//! on.
//!
//! # Parameter-entity conservatism
//!
//! A non-validating processor need not read declarations that follow an
//! unread parameter-entity reference (XML 1.0 §5.1). Because fastxml does not
//! expand parameter entities, once a parameter-entity reference appears at the
//! declaration-separator level — or inside a particular declaration — the
//! structure of the affected declarations may be invisible. In those cases the
//! checker validates only character legality and quote/paren balance and
//! otherwise *accepts*. When a construct is ambiguous, the checker accepts.
//!
//! The input is the exact text of quick-xml's `DocType` event: the leading
//! `<!DOCTYPE` and the trailing `>` are already stripped. The caller only
//! invokes this function when the internal subset (if any) is complete; a
//! subset that quick-xml truncated at a `>` inside a quoted literal is skipped.

use super::error::ParseError;
use super::wellformed::{check_char_refs, check_name, is_name_char, is_name_start_char};

type R = Result<(), ParseError>;

fn err(message: impl Into<String>) -> ParseError {
    ParseError::NotWellFormed {
        message: message.into(),
    }
}

/// Checks the raw `DOCTYPE` event text for well-formedness.
pub(crate) fn check_doctype(raw: &str) -> R {
    let mut c = Cursor::new(raw);
    c.skip_s();
    c.parse_name("document type name")?;
    c.skip_s();
    // Optional ExternalID (SYSTEM/PUBLIC, keywords case-sensitive).
    if c.starts_with("SYSTEM") || c.starts_with("PUBLIC") {
        parse_external_id(&mut c)?;
        c.skip_s();
    }
    // Optional internal subset.
    if c.eat('[') {
        parse_internal_subset(&mut c)?;
        c.skip_s();
    }
    if !c.at_end() {
        return Err(err("unexpected content in the DOCTYPE declaration"));
    }
    Ok(())
}

/// A char cursor over a `&str`, tracking a byte offset.
struct Cursor<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Cursor<'a> {
    fn new(s: &'a str) -> Self {
        Self { s, i: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.i += c.len_utf8();
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_str(&mut self, kw: &str) -> bool {
        if self.rest().starts_with(kw) {
            self.i += kw.len();
            true
        } else {
            false
        }
    }

    fn starts_with(&self, kw: &str) -> bool {
        self.rest().starts_with(kw)
    }

    /// Skips XML white space, returning whether any was consumed.
    fn skip_s(&mut self) -> bool {
        let mut any = false;
        while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
            self.bump();
            any = true;
        }
        any
    }

    fn require_s(&mut self, ctx: &str) -> R {
        if self.skip_s() {
            Ok(())
        } else {
            Err(err(format!("white space is required {ctx}")))
        }
    }

    /// Parses a `Name`, returning the matched slice.
    fn parse_name(&mut self, ctx: &str) -> Result<&'a str, ParseError> {
        let start = self.i;
        match self.peek() {
            Some(c) if is_name_start_char(c) => self.bump(),
            Some(c) => {
                return Err(err(format!(
                    "illegal {ctx} start character U+{:04X}",
                    c as u32
                )));
            }
            None => return Err(err(format!("expected {ctx}"))),
        }
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(&self.s[start..self.i])
    }

    /// Parses an `Nmtoken` (one or more `NameChar`s), returning the slice.
    fn parse_nmtoken(&mut self, ctx: &str) -> Result<&'a str, ParseError> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                self.bump();
            } else {
                break;
            }
        }
        if self.i == start {
            return Err(err(format!("expected {ctx}")));
        }
        Ok(&self.s[start..self.i])
    }

    /// Consumes a quoted literal (`"…"` or `'…'`), returning the inner text.
    fn parse_quoted(&mut self, ctx: &str) -> Result<&'a str, ParseError> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(err(format!("expected a quoted {ctx}"))),
        };
        self.bump();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == quote {
                let inner = &self.s[start..self.i];
                self.bump();
                return Ok(inner);
            }
            self.bump();
        }
        Err(err(format!("unterminated {ctx}")))
    }
}

/// `ExternalID ::= 'SYSTEM' S SystemLiteral | 'PUBLIC' S PubidLiteral S SystemLiteral`.
fn parse_external_id(c: &mut Cursor<'_>) -> R {
    if c.eat_str("SYSTEM") {
        c.require_s("after the SYSTEM keyword")?;
        c.parse_quoted("system identifier")?;
    } else if c.eat_str("PUBLIC") {
        c.require_s("after the PUBLIC keyword")?;
        let pubid = c.parse_quoted("public identifier")?;
        check_pubid(pubid)?;
        c.require_s("between the public and system identifiers")?;
        c.parse_quoted("system identifier")?;
    } else {
        return Err(err("expected SYSTEM or PUBLIC in the external identifier"));
    }
    Ok(())
}

/// Validates the characters of a `PubidLiteral` (`PubidChar`, XML 1.0 P13).
fn check_pubid(s: &str) -> R {
    for c in s.chars() {
        if !is_pubid_char(c) {
            return Err(err(format!(
                "illegal character U+{:04X} in the public identifier",
                c as u32
            )));
        }
    }
    Ok(())
}

/// `PubidChar ::= #x20 | #xD | #xA | [a-zA-Z0-9] | [-'()+,./:=?;!*#@$_%]`.
fn is_pubid_char(c: char) -> bool {
    matches!(c,
        ' ' | '\r' | '\n'
        | 'a'..='z' | 'A'..='Z' | '0'..='9'
        | '-' | '\'' | '(' | ')' | '+' | ',' | '.' | '/' | ':' | '='
        | '?' | ';' | '!' | '*' | '#' | '@' | '$' | '_' | '%')
}

/// Iterates the internal subset items until the closing `]`.
///
/// `intSubset ::= (markupdecl | DeclSep)*`, where a `DeclSep` is white space or
/// a parameter-entity reference. Once a parameter-entity reference is seen at
/// this level, later declarations may be structurally invisible, so they are
/// validated leniently (see the module docs).
fn parse_internal_subset(c: &mut Cursor<'_>) -> R {
    let mut pe_used = false;
    loop {
        c.skip_s();
        match c.peek() {
            None => return Err(err("unterminated DOCTYPE internal subset")),
            Some(']') => {
                c.bump();
                return Ok(());
            }
            Some('%') => {
                parse_pe_reference(c)?;
                pe_used = true;
            }
            Some('<') => parse_markup_decl(c, pe_used)?,
            Some(ch) => {
                return Err(err(format!(
                    "unexpected character U+{:04X} in the internal subset",
                    ch as u32
                )));
            }
        }
    }
}

/// `PEReference ::= '%' Name ';'`.
fn parse_pe_reference(c: &mut Cursor<'_>) -> R {
    c.bump(); // '%'
    c.parse_name("parameter-entity name")?;
    if !c.eat(';') {
        return Err(err("a parameter-entity reference must end with ';'"));
    }
    Ok(())
}

/// Dispatches on a markup declaration, comment, PI, or (illegal) conditional
/// section starting at `<`.
fn parse_markup_decl(c: &mut Cursor<'_>, pe_used: bool) -> R {
    if c.starts_with("<?") {
        return parse_pi(c);
    }
    if c.starts_with("<!--") {
        return parse_comment(c);
    }
    if c.starts_with("<![") {
        return Err(err(
            "a conditional section is not allowed in the internal DTD subset",
        ));
    }
    if c.eat_str("<!ELEMENT") {
        return finish_decl(c, pe_used, parse_element_decl);
    }
    if c.eat_str("<!ATTLIST") {
        return finish_decl(c, pe_used, parse_attlist_decl);
    }
    if c.eat_str("<!ENTITY") {
        return finish_decl(c, pe_used, parse_entity_decl);
    }
    if c.eat_str("<!NOTATION") {
        return finish_decl(c, pe_used, parse_notation_decl);
    }
    Err(err("expected a markup declaration in the internal subset"))
}

/// Extracts a declaration body up to its top-level `>` (honoring quotes), then
/// validates it with `parse` — unless a parameter-entity reference is in force,
/// in which case only the extraction (which proves quote balance) is required.
fn finish_decl(c: &mut Cursor<'_>, pe_used: bool, parse: fn(&str) -> R) -> R {
    let body = take_decl_body(c)?;
    if pe_used || has_pe_reference(body) {
        return Ok(());
    }
    parse(body)
}

/// Consumes text up to and including the top-level `>` that closes a markup
/// declaration, returning the body before it.
fn take_decl_body<'a>(c: &mut Cursor<'a>) -> Result<&'a str, ParseError> {
    let start = c.i;
    let mut quote: Option<char> = None;
    while let Some(ch) = c.peek() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => {
                    let body = &c.s[start..c.i];
                    c.bump();
                    return Ok(body);
                }
                _ => {}
            },
        }
        c.bump();
    }
    Err(err("unterminated markup declaration"))
}

/// True if `body` contains a parameter-entity reference (`%` immediately
/// followed by a name-start character), inside or outside quotes.
fn has_pe_reference(body: &str) -> bool {
    let mut chars = body.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == '%'
            && let Some((_, next)) = chars.peek()
            && is_name_start_char(*next)
        {
            return true;
        }
    }
    false
}

/// `PI ::= '<?' PITarget (S (Char* - (Char* '?>' Char*)))? '?>'`.
fn parse_pi(c: &mut Cursor<'_>) -> R {
    // Consume up to '?>'.
    let after = &c.rest()[2..]; // skip '<?'
    let Some(end) = after.find("?>") else {
        return Err(err("unterminated processing instruction"));
    };
    let inner = &after[..end];
    c.i += 2 + end + 2;
    let target = inner.split([' ', '\t', '\r', '\n']).next().unwrap_or("");
    if target.is_empty() {
        return Err(err("a processing instruction must have a target"));
    }
    check_name(target, "processing-instruction target")?;
    if target.eq_ignore_ascii_case("xml") {
        return Err(err(format!(
            "'{target}' is a reserved processing-instruction target"
        )));
    }
    Ok(())
}

/// `Comment ::= '<!--' ((Char - '-') | ('-' (Char - '-')))* '-->'`.
fn parse_comment(c: &mut Cursor<'_>) -> R {
    let after = &c.rest()[4..]; // skip '<!--'
    let Some(end) = after.find("-->") else {
        return Err(err("unterminated comment"));
    };
    let inner = &after[..end];
    c.i += 4 + end + 3;
    if inner.contains("--") {
        return Err(err("'--' is not allowed inside a comment"));
    }
    Ok(())
}

/// `elementdecl ::= '<!ELEMENT' S Name S contentspec S? '>'`.
fn parse_element_decl(body: &str) -> R {
    let mut c = Cursor::new(body);
    c.require_s("after ELEMENT")?;
    c.parse_name("element type name")?;
    c.require_s("before the content specification")?;
    parse_contentspec(&mut c)?;
    c.skip_s();
    if !c.at_end() {
        return Err(err("trailing content in an element declaration"));
    }
    Ok(())
}

/// `contentspec ::= 'EMPTY' | 'ANY' | Mixed | children`.
fn parse_contentspec(c: &mut Cursor<'_>) -> R {
    if c.eat_str("EMPTY") || c.eat_str("ANY") {
        return Ok(());
    }
    if c.peek() != Some('(') {
        return Err(err("expected EMPTY, ANY, or a content model"));
    }
    // Distinguish Mixed (`(#PCDATA…`) from children.
    let mut look = Cursor::new(c.rest());
    look.bump(); // '('
    look.skip_s();
    if look.starts_with("#PCDATA") {
        parse_mixed(c)
    } else {
        parse_children(c)
    }
}

/// `Mixed ::= '(' S? '#PCDATA' (S? '|' S? Name)* S? ')*' | '(' S? '#PCDATA' S? ')'`.
///
/// A trailing `*` is also tolerated after the bare `(#PCDATA)` form, which some
/// documents in the wild use.
fn parse_mixed(c: &mut Cursor<'_>) -> R {
    c.eat('('); // '('
    c.skip_s();
    if !c.eat_str("#PCDATA") {
        return Err(err("expected #PCDATA in a mixed content model"));
    }
    let mut had_names = false;
    loop {
        c.skip_s();
        if c.eat('|') {
            had_names = true;
            c.skip_s();
            c.parse_name("element name in a mixed content model")?;
        } else {
            break;
        }
    }
    c.skip_s();
    if !c.eat(')') {
        return Err(err("expected ')' to close a mixed content model"));
    }
    if had_names {
        // With alternatives the closing delimiter must be ')*'.
        if !c.eat('*') {
            return Err(err(
                "a mixed content model with element names must end with ')*'",
            ));
        }
    } else {
        // Bare (#PCDATA): tolerate an optional '*'.
        c.eat('*');
    }
    Ok(())
}

/// `children ::= (choice | seq) ('?' | '*' | '+')?` with balanced groups and a
/// consistent separator inside each group.
fn parse_children(c: &mut Cursor<'_>) -> R {
    parse_cp(c)?;
    Ok(())
}

/// `cp ::= (Name | choice | seq) ('?' | '*' | '+')?`.
fn parse_cp(c: &mut Cursor<'_>) -> R {
    if c.peek() == Some('(') {
        parse_group(c)?;
    } else {
        c.parse_name("element name in a content model")?;
    }
    // Optional occurrence indicator.
    if matches!(c.peek(), Some('?' | '*' | '+')) {
        c.bump();
    }
    Ok(())
}

/// Parses a `choice` or `seq` group: `'(' S? cp ( S? sep S? cp )* S? ')'` where
/// `sep` is uniformly `|` (choice) or `,` (seq).
fn parse_group(c: &mut Cursor<'_>) -> R {
    c.eat('('); // '('
    c.skip_s();
    parse_cp(c)?;
    c.skip_s();
    let sep = match c.peek() {
        Some(s @ ('|' | ',')) => Some(s),
        _ => None,
    };
    if let Some(sep) = sep {
        loop {
            c.skip_s();
            match c.peek() {
                Some(s) if s == sep => {
                    c.bump();
                }
                Some('|') | Some(',') => {
                    return Err(err(
                        "a content-model group must use a single connector (',' or '|')",
                    ));
                }
                _ => break,
            }
            c.skip_s();
            parse_cp(c)?;
            c.skip_s();
        }
    }
    c.skip_s();
    if !c.eat(')') {
        return Err(err("expected ')' to close a content-model group"));
    }
    Ok(())
}

/// `AttlistDecl ::= '<!ATTLIST' S Name AttDef* S? '>'`.
fn parse_attlist_decl(body: &str) -> R {
    let mut c = Cursor::new(body);
    c.require_s("after ATTLIST")?;
    c.parse_name("element type name in an attribute-list declaration")?;
    // AttDef* — each begins with white space.
    loop {
        let had_s = c.skip_s();
        if c.at_end() {
            break;
        }
        if !had_s {
            return Err(err(
                "white space is required before an attribute definition",
            ));
        }
        if c.at_end() {
            break;
        }
        parse_att_def(&mut c)?;
    }
    Ok(())
}

/// `AttDef ::= S Name S AttType S DefaultDecl`.
fn parse_att_def(c: &mut Cursor<'_>) -> R {
    c.parse_name("attribute name")?;
    c.require_s("before the attribute type")?;
    parse_att_type(c)?;
    c.require_s("before the attribute default")?;
    parse_default_decl(c)?;
    Ok(())
}

/// `AttType ::= StringType | TokenizedType | EnumeratedType`.
fn parse_att_type(c: &mut Cursor<'_>) -> R {
    const TOKENIZED: [&str; 8] = [
        "CDATA", "IDREFS", "IDREF", "ID", "ENTITY", "ENTITIES", "NMTOKENS", "NMTOKEN",
    ];
    if c.eat_str("NOTATION") {
        c.require_s("after NOTATION")?;
        return parse_name_group(c);
    }
    for kw in TOKENIZED {
        if c.eat_str(kw) {
            return Ok(());
        }
    }
    // Enumeration.
    if c.peek() == Some('(') {
        return parse_enumeration(c);
    }
    Err(err("expected an attribute type"))
}

/// `NotationType ::= 'NOTATION' S '(' S? Name (S? '|' S? Name)* S? ')'`.
fn parse_name_group(c: &mut Cursor<'_>) -> R {
    if !c.eat('(') {
        return Err(err("expected '(' after NOTATION"));
    }
    loop {
        c.skip_s();
        c.parse_name("notation name")?;
        c.skip_s();
        if c.eat('|') {
            continue;
        }
        break;
    }
    if !c.eat(')') {
        return Err(err("expected ')' to close a NOTATION type"));
    }
    Ok(())
}

/// `Enumeration ::= '(' S? Nmtoken (S? '|' S? Nmtoken)* S? ')'`.
fn parse_enumeration(c: &mut Cursor<'_>) -> R {
    if !c.eat('(') {
        return Err(err("expected '(' to open an enumeration"));
    }
    loop {
        c.skip_s();
        c.parse_nmtoken("enumeration token")?;
        c.skip_s();
        if c.eat('|') {
            continue;
        }
        break;
    }
    if !c.eat(')') {
        return Err(err("expected ')' to close an enumeration"));
    }
    Ok(())
}

/// `DefaultDecl ::= '#REQUIRED' | '#IMPLIED' | (('#FIXED' S)? AttValue)`.
fn parse_default_decl(c: &mut Cursor<'_>) -> R {
    if c.eat_str("#REQUIRED") || c.eat_str("#IMPLIED") {
        return Ok(());
    }
    if c.eat_str("#FIXED") {
        c.require_s("after #FIXED")?;
    }
    parse_att_value(c)
}

/// `AttValue ::= '"' ([^<&"] | Reference)* '"' | "'" ([^<&'] | Reference)* "'"`.
fn parse_att_value(c: &mut Cursor<'_>) -> R {
    let value = c.parse_quoted("attribute default value")?;
    if value.contains('<') {
        return Err(err("'<' is not allowed in an attribute default value"));
    }
    check_char_refs(value, "attribute default value")?;
    Ok(())
}

/// `EntityDecl ::= GEDecl | PEDecl`.
///
/// `GEDecl ::= '<!ENTITY' S Name S EntityDef S? '>'`
/// `PEDecl ::= '<!ENTITY' S '%' S Name S PEDef S? '>'`
fn parse_entity_decl(body: &str) -> R {
    let mut c = Cursor::new(body);
    c.require_s("after ENTITY")?;
    let is_pe = c.eat('%');
    if is_pe {
        c.require_s("after '%' in a parameter-entity declaration")?;
    }
    c.parse_name("entity name")?;
    c.require_s("before the entity definition")?;
    // EntityValue | ExternalID [NDataDecl]
    if matches!(c.peek(), Some('"' | '\'')) {
        let value = c.parse_quoted("entity value")?;
        check_char_refs(value, "entity value")?;
    } else {
        parse_external_id(&mut c)?;
        if !is_pe {
            // Optional NDataDecl for a general entity.
            let mut look = Cursor::new(c.rest());
            if look.skip_s() && look.eat_str("NDATA") {
                c.skip_s();
                c.eat_str("NDATA");
                c.require_s("after NDATA")?;
                c.parse_name("notation name")?;
            }
        }
    }
    c.skip_s();
    if !c.at_end() {
        return Err(err("trailing content in an entity declaration"));
    }
    Ok(())
}

/// `NotationDecl ::= '<!NOTATION' S Name S (ExternalID | PublicID) S? '>'`,
/// where `PublicID ::= 'PUBLIC' S PubidLiteral`.
fn parse_notation_decl(body: &str) -> R {
    let mut c = Cursor::new(body);
    c.require_s("after NOTATION")?;
    c.parse_name("notation name")?;
    c.require_s("before the notation identifier")?;
    if c.eat_str("SYSTEM") {
        c.require_s("after the SYSTEM keyword")?;
        c.parse_quoted("system identifier")?;
    } else if c.eat_str("PUBLIC") {
        c.require_s("after the PUBLIC keyword")?;
        let pubid = c.parse_quoted("public identifier")?;
        check_pubid(pubid)?;
        // A system identifier is optional for a notation (PublicID form).
        let mut look = Cursor::new(c.rest());
        if look.skip_s() && matches!(look.peek(), Some('"' | '\'')) {
            c.require_s("before the system identifier")?;
            c.parse_quoted("system identifier")?;
        }
    } else {
        return Err(err("expected SYSTEM or PUBLIC in a notation declaration"));
    }
    c.skip_s();
    if !c.at_end() {
        return Err(err("trailing content in a notation declaration"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) {
        assert!(check_doctype(raw).is_ok(), "expected accept: {raw:?}");
    }
    fn bad(raw: &str) {
        assert!(check_doctype(raw).is_err(), "expected reject: {raw:?}");
    }

    #[test]
    fn accepts_minimal_and_external_ids() {
        ok("doc");
        ok("doc SYSTEM \"foo.dtd\"");
        ok("doc PUBLIC \"-//A//B\" \"foo.dtd\"");
        ok("doc PUBLIC 'The latest version' 'student.dtd'[\n]");
    }

    #[test]
    fn accepts_common_declarations() {
        ok("s [ <!ELEMENT s (#PCDATA)> ]");
        ok("s [ <!ELEMENT s (#PCDATA)*> ]");
        ok("s [ <!ELEMENT a (b|c|d)+> <!ELEMENT b ANY> ]");
        ok("s [ <!ELEMENT r (a,b?,(c|d)*)> ]");
        ok("s [ <!ELEMENT r EMPTY> <!ATTLIST r x CDATA #REQUIRED y (a|b) 'a'> ]");
        ok("s [ <!ENTITY e \"val\"> <!ENTITY % p \"x\"> ]");
        ok("s [ <!ENTITY ext SYSTEM \"e.gif\" NDATA gif> ]");
        ok("s [ <!NOTATION n SYSTEM \"x\"> <!NOTATION m PUBLIC \"-//p//id\"> ]");
        ok("s [ <?target data?> <!-- a comment --> ]");
    }

    #[test]
    fn rejects_bad_pi_target_in_subset() {
        bad("s [ <? ?> ]");
        bad("s [ <?xml foo?> ]");
        bad("s [ <?1bad x?> ]");
    }

    #[test]
    fn rejects_conditional_section_and_bad_comment() {
        bad("s [ <![INCLUDE[ <!ELEMENT a ANY> ]]> ]");
        bad("s [ <!-- a -- b --> ]");
    }

    #[test]
    fn rejects_malformed_declarations() {
        bad("s [ <!ELEMENT> ]");
        bad("s [ <!ELEMENT s > ]");
        bad("s [ <!ELEMENT s (a,b|c)> ]"); // mixed connectors
        bad("s [ <!ATTLIST s x BOGUS #IMPLIED> ]");
        bad("s [ <!ENTITY> ]");
        bad("s [ <!NOTATION n \"x\"> ]"); // missing keyword
        bad("s [ <!WHAT foo> ]");
    }

    #[test]
    fn rejects_bad_pubid_and_external_keyword_case() {
        bad("doc PUBLIC \"a`b\" \"foo.dtd\""); // backtick not a PubidChar
        bad("doc system \"foo.dtd\""); // lowercase keyword
    }

    #[test]
    fn accepts_when_parameter_entity_in_play() {
        // A PE reference at declaration-separator level makes later declarations
        // structurally invisible: accept.
        ok("s [ %pe; <!ELEMENT this could be anything> ]");
        // A declaration whose body uses a PE reference is validated leniently.
        ok("s [ <!ELEMENT r %content;> ]");
    }
}
