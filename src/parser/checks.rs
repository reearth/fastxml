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
use smallvec::SmallVec;

use super::error::ParseError;
use super::wellformed::{check_char_refs, check_chars, check_name, check_text};
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
    /// Whether any content (text, comment, PI, DOCTYPE, or element) has been
    /// seen. The XML declaration is only well-formed as the very first thing in
    /// the document.
    document_started: bool,
    /// Namespace declarations in scope, one entry per open element (innermost
    /// last). Each entry maps a prefix (`""` for the default namespace) to its
    /// URI. Used to enforce namespace well-formedness.
    ns_stack: Vec<Vec<(String, String)>>,
}

impl Default for WellformedChecker {
    fn default() -> Self {
        Self {
            state: DocState::Prolog,
            root_seen: false,
            doctype_seen: false,
            dtd_open: false,
            document_started: false,
            ns_stack: Vec::new(),
        }
    }
}

/// The namespace name bound to the reserved `xml` prefix.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

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

/// Finds the byte offset of the `[` that opens the internal subset, scanning
/// outside quoted literals so a `[` inside a public/system identifier is not
/// mistaken for it. Returns `None` when there is no internal subset.
fn subset_open(raw: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in raw.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'[' => return Some(i),
                _ => {}
            },
        }
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

/// Validates the raw body of an XML declaration, i.e. quick-xml's `Decl` event
/// text `xml VersionInfo EncodingDecl? SDDecl? S?` (the surrounding `<?`/`?>`
/// are already stripped).
///
/// ```text
/// VersionInfo  ::= S 'version' Eq ("'" VersionNum "'" | '"' VersionNum '"')
/// VersionNum   ::= '1.' [0-9]+
/// EncodingDecl ::= S 'encoding' Eq ('"' EncName '"' | "'" EncName "'")
/// EncName      ::= [A-Za-z] ([A-Za-z0-9._] | '-')*
/// SDDecl       ::= S 'standalone' Eq (('"' ('yes' | 'no') '"') | ("'" ('yes' | 'no') "'"))
/// ```
fn check_declaration(raw: &str) -> std::result::Result<(), ParseError> {
    let rest = raw
        .strip_prefix("xml")
        .ok_or_else(|| not_wf("malformed XML declaration"))?;

    // VersionInfo (required).
    let rest = require_leading_s(rest, "before 'version' in the XML declaration")?;
    let rest = rest
        .strip_prefix("version")
        .ok_or_else(|| not_wf("the XML declaration must begin with 'version'"))?;
    let (rest, version) = parse_eq_quoted(rest, "version")?;
    if !is_version_num(version) {
        return Err(not_wf(format!("unsupported XML version '{version}'")));
    }

    // EncodingDecl (optional).
    let rest = match pseudo_attr(rest, "encoding")? {
        Some((after, enc)) => {
            if !is_enc_name(enc) {
                return Err(not_wf(format!("illegal encoding name '{enc}'")));
            }
            after
        }
        None => rest,
    };

    // SDDecl (optional).
    let rest = match pseudo_attr(rest, "standalone")? {
        Some((after, sd)) => {
            if sd != "yes" && sd != "no" {
                return Err(not_wf(format!(
                    "the standalone declaration must be 'yes' or 'no', not '{sd}'"
                )));
            }
            after
        }
        None => rest,
    };

    if !rest.trim_start_matches(is_xml_space).is_empty() {
        return Err(not_wf("unexpected content in the XML declaration"));
    }
    Ok(())
}

/// Requires at least one `S` at the start of `s`, returning the remainder.
fn require_leading_s<'a>(s: &'a str, ctx: &str) -> std::result::Result<&'a str, ParseError> {
    let trimmed = s.trim_start_matches(is_xml_space);
    if trimmed.len() == s.len() {
        return Err(not_wf(format!("white space is required {ctx}")));
    }
    Ok(trimmed)
}

/// Parses `Eq ('"' value '"' | "'" value "'")`, returning `(rest, value)`.
fn parse_eq_quoted<'a>(
    s: &'a str,
    name: &str,
) -> std::result::Result<(&'a str, &'a str), ParseError> {
    let s = s.trim_start_matches(is_xml_space);
    let s = s
        .strip_prefix('=')
        .ok_or_else(|| not_wf(format!("expected '=' after '{name}'")))?;
    let s = s.trim_start_matches(is_xml_space);
    let quote = match s.chars().next() {
        Some(q @ ('"' | '\'')) => q,
        _ => return Err(not_wf(format!("the value of '{name}' must be quoted"))),
    };
    let after = &s[quote.len_utf8()..];
    match after.find(quote) {
        Some(end) => Ok((&after[end + quote.len_utf8()..], &after[..end])),
        None => Err(not_wf(format!("unterminated value for '{name}'"))),
    }
}

/// Recognizes an optional pseudo-attribute `S 'name' Eq quoted`, requiring the
/// leading `S`. Returns `(rest, value)` when present, or `None` when the next
/// token is not `name`.
fn pseudo_attr<'a>(
    s: &'a str,
    name: &str,
) -> std::result::Result<Option<(&'a str, &'a str)>, ParseError> {
    let trimmed = s.trim_start_matches(is_xml_space);
    if !trimmed.starts_with(name) {
        return Ok(None);
    }
    if trimmed.len() == s.len() {
        return Err(not_wf(format!("white space is required before '{name}'")));
    }
    let (rest, value) = parse_eq_quoted(&trimmed[name.len()..], name)?;
    Ok(Some((rest, value)))
}

/// `VersionNum ::= '1.' [0-9]+`.
fn is_version_num(v: &str) -> bool {
    match v.strip_prefix("1.") {
        Some(digits) => !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Validates an `xmlns:prefix="uri"` declaration.
fn check_ns_declaration(prefix: &str, uri: &str) -> std::result::Result<(), ParseError> {
    if prefix.is_empty() {
        return Err(not_wf(
            "a namespace prefix declared with 'xmlns:' must not be empty",
        ));
    }
    if !is_ncname(prefix) {
        return Err(not_wf(format!("illegal namespace prefix '{prefix}'")));
    }
    match prefix {
        "xmlns" => Err(not_wf("the prefix 'xmlns' must not be declared")),
        "xml" if uri != XML_NAMESPACE => Err(not_wf(
            "the prefix 'xml' may only be bound to the XML namespace",
        )),
        // Unbinding a prefix (empty URI) is an XML Namespaces 1.1 feature and is
        // not well-formed in a 1.0 document.
        _ if uri.is_empty() => Err(not_wf(format!(
            "a namespace prefix must not be unbound (xmlns:{prefix}=\"\")"
        ))),
        _ => Ok(()),
    }
}

/// True when `s` is a non-empty `NCName` (a `Name` with no colon).
fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c != ':' && super::wellformed::is_name_start_char(c) => {}
        _ => return false,
    }
    chars.all(|c| c != ':' && super::wellformed::is_name_char(c))
}

/// `EncName ::= [A-Za-z] ([A-Za-z0-9._] | '-')*`.
fn is_enc_name(e: &str) -> bool {
    let mut chars = e.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl WellformedChecker {
    /// Creates a checker for a fresh document.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Checks a start (or empty-element) tag: the element `Name`, every
    /// attribute's `Name` and raw value, and the document-structure position.
    pub(crate) fn start(&mut self, e: &BytesStart<'_>) -> Result<()> {
        self.document_started = true;
        let qname = std::str::from_utf8(e.name().into_inner())?;
        check_name(qname, "element name")?;

        // Single pass over the attributes: character/reference checks, collect
        // namespace declarations into a new scope, and remember prefixed
        // attribute names (borrowed) for the expanded-name uniqueness check.
        // `scope` only allocates when this element declares a namespace, and
        // the prefixed-name list stays on the stack for the common few-attribute
        // case, so the checks add no per-element heap traffic in typical input.
        let mut scope: Vec<(String, String)> = Vec::new();
        let mut prefixed: SmallVec<[&str; 8]> = SmallVec::new();
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
            check_char_refs(value, "attribute value")?;
            // `<` is forbidden in an attribute value (the AttValue production
            // excludes it); a literal here can never be a character reference.
            if value.contains('<') {
                return Err(not_wf(format!(
                    "'<' is not allowed in the value of attribute '{key}'"
                ))
                .into());
            }
            if key == "xmlns" {
                scope.push((String::new(), value.to_string()));
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                check_ns_declaration(prefix, value)?;
                scope.push((prefix.to_string(), value.to_string()));
            } else if key.contains(':') {
                prefixed.push(key);
            }
        }
        self.ns_stack.push(scope);

        // Reject two attributes that resolve to the same expanded name (same
        // namespace URI and local name) even when written with different
        // prefixes. Other namespace constraints (QName syntax, prefix binding)
        // are intentionally not enforced here: the W3C XML 1.0 conformance
        // suite treats colons as ordinary name characters, so a namespace-aware
        // QName check would reject documents that are well-formed XML 1.0.
        self.check_duplicate_expanded_attrs(&prefixed)?;

        self.open_element()?;
        Ok(())
    }

    /// Rejects two attributes sharing a namespace URI and local name. Only
    /// attributes whose prefix is actually bound participate; attributes with
    /// an unbound prefix keep their literal name, and quick-xml already rejects
    /// literal duplicates.
    fn check_duplicate_expanded_attrs(&self, prefixed: &[&str]) -> Result<()> {
        if prefixed.len() < 2 {
            return Ok(());
        }
        let mut expanded: SmallVec<[(&str, &str); 8]> = SmallVec::new();
        for key in prefixed {
            let Some((prefix, local)) = key.split_once(':') else {
                continue;
            };
            let Some(uri) = self.resolve(prefix) else {
                continue;
            };
            if expanded.contains(&(uri, local)) {
                return Err(not_wf(format!(
                    "duplicate attribute '{key}' after namespace resolution"
                ))
                .into());
            }
            expanded.push((uri, local));
        }
        Ok(())
    }

    /// Resolves a prefix to its namespace URI in the current scope, or `None`
    /// when it is unbound. `xml` is always bound; a prefix bound to the empty
    /// string is treated as unbound.
    fn resolve(&self, prefix: &str) -> Option<&str> {
        if prefix == "xml" {
            return Some(XML_NAMESPACE);
        }
        for scope in self.ns_stack.iter().rev() {
            if let Some((_, uri)) = scope.iter().find(|(p, _)| p == prefix) {
                return if uri.is_empty() { None } else { Some(uri) };
            }
        }
        None
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
        // Leave this element's namespace scope.
        self.ns_stack.pop();
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
        // White space in the prolog does not itself begin the document, but any
        // text preceding a declaration still means the declaration is not first;
        // marking it here keeps the declaration-position check correct.
        self.document_started = true;
        // When quick-xml truncated the DOCTYPE at a `>` inside a quoted literal,
        // the remaining internal subset (up to its closing `]`) arrives as text.
        // Consume it as DTD rather than judging it as prolog character data.
        if self.dtd_open {
            check_chars(raw, "text content")?;
            if let Some(end) = internal_subset_end(raw) {
                self.dtd_open = false;
                // Anything after the `]` and the DOCTYPE's `>` is prolog text.
                let tail = &raw[end + 1..];
                let after = tail.strip_prefix('>').unwrap_or(tail);
                return self.text(after);
            }
            return Ok(());
        }
        // Single pass: Char production, no literal `]]>`, and well-formed
        // character references.
        check_text(raw, "text content")?;
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
        self.document_started = true;
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
        self.document_started = true;
        check_chars(raw, "comment")?;
        // `--` must not appear inside a comment (XML 1.0 P15). The DOM engine's
        // tokenizer already rejects this; enforcing it here keeps the streaming
        // engine in lockstep.
        if raw.contains("--") {
            return Err(not_wf("'--' is not allowed inside a comment").into());
        }
        Ok(())
    }

    /// Checks a processing instruction's raw body (target plus data).
    pub(crate) fn pi(&mut self, raw: &str) -> Result<()> {
        self.document_started = true;
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

    /// Checks an XML declaration's raw body (`xml` followed by the version,
    /// optional encoding, and optional standalone pseudo-attributes).
    pub(crate) fn decl(&mut self, raw: &str) -> Result<()> {
        if self.document_started {
            return Err(not_wf(
                "the XML declaration must be at the very beginning of the document",
            )
            .into());
        }
        self.document_started = true;
        check_declaration(raw)?;
        Ok(())
    }

    /// Checks a `DOCTYPE` declaration's raw body (name, external id, and
    /// internal subset, exactly as the tokenizer delivered it).
    pub(crate) fn doctype(&mut self, raw: &str) -> Result<()> {
        self.document_started = true;
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
        // complete, validate the whole declaration's grammar. The subset opener
        // must be located outside quoted literals so a `[` inside a public or
        // system identifier is not mistaken for it.
        match subset_open(raw) {
            Some(open) if internal_subset_end(&raw[open + 1..]).is_none() => {
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

    #[test]
    fn declaration_accepts_well_formed() {
        for raw in [
            "xml version=\"1.0\"",
            "xml version='1.0'",
            "xml version=\"1.1\"",
            "xml version=\"1.0\" encoding=\"UTF-8\"",
            "xml version=\"1.0\" standalone=\"yes\"",
            "xml version=\"1.0\" encoding=\"ISO-8859-1\" standalone='no' ",
        ] {
            assert!(check_declaration(raw).is_ok(), "expected accept: {raw:?}");
        }
    }

    #[test]
    fn declaration_rejects_malformed() {
        for raw in [
            "xml",                                                       // no version
            "xml encoding=\"UTF-8\"",                                    // version missing
            "xml version=\"2.0\"",                                       // bad VersionNum
            "xml version=\"1.0\"standalone=\"yes\"",                     // no S before standalone
            "xml version=\"1.0\" standalone=\"maybe\"",                  // bad standalone value
            "xml version=\"1.0\" standalone=\"yes\" encoding=\"UTF-8\"", // wrong order
            "xml version=\"1.0\" encoding=\"UTF 8\"",                    // bad EncName
            "xml version = 1.0",                                         // unquoted value
            "xml version=\"1.0\" foo=\"bar\"",                           // stray pseudo-attr
        ] {
            assert!(check_declaration(raw).is_err(), "expected reject: {raw:?}");
        }
    }

    #[test]
    fn ns_declaration_rules() {
        assert!(check_ns_declaration("a", "http://example.org/a").is_ok());
        assert!(check_ns_declaration("xml", XML_NAMESPACE).is_ok());
        // 'xml' bound to the wrong URI.
        assert!(check_ns_declaration("xml", "http://wrong").is_err());
        // 'xmlns' must not be declared.
        assert!(check_ns_declaration("xmlns", "http://example.org").is_err());
        // Empty prefix (`xmlns:`) and prefix unbinding are illegal in 1.0.
        assert!(check_ns_declaration("", "http://example.org").is_err());
        assert!(check_ns_declaration("a", "").is_err());
    }

    #[test]
    fn ncname_recognition() {
        assert!(is_ncname("a") && is_ncname("_x.y-z0"));
        assert!(!is_ncname("") && !is_ncname("a:b") && !is_ncname("0a"));
    }

    #[test]
    fn version_num_and_enc_name() {
        assert!(is_version_num("1.0") && is_version_num("1.10"));
        assert!(!is_version_num("2.0") && !is_version_num("1.") && !is_version_num("1"));
        assert!(is_enc_name("UTF-8") && is_enc_name("a"));
        assert!(!is_enc_name("8bit") && !is_enc_name("") && !is_enc_name("x y"));
    }
}
