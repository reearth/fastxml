//! Internal DTD subset parsing for general entity declarations.
//!
//! quick-xml hands the DOCTYPE declaration over as raw text; this module
//! extracts `<!ENTITY name "value">` declarations from the internal subset
//! so entity references in content and attribute values can be resolved
//! (by [`super::expand`]). External (SYSTEM/PUBLIC) and parameter (`%`)
//! entities are skipped: references to external entities are rejected as
//! unknown, and markup reached through parameter entities is not read.

use std::collections::{HashMap, HashSet};

/// Parses the internal DTD subset of a DOCTYPE declaration into a map of
/// general entity name → replacement text (character references expanded,
/// entity references kept for expansion at the point of use; see
/// [`super::expand`]).
#[cfg(test)]
pub(crate) fn parse_internal_entities(doctype: &str) -> HashMap<String, String> {
    parse_entity_declarations(doctype).replacements
}

/// The general entities declared in a DOCTYPE's internal subset.
#[derive(Debug, Default)]
pub(crate) struct EntityDeclarations {
    /// Internal entities: name → replacement text (see
    /// [`parse_internal_entities`]).
    pub replacements: HashMap<String, String>,
    /// External parsed entities (`SYSTEM`/`PUBLIC` without `NDATA`), whose
    /// replacement text is not read.
    pub external: HashSet<String>,
}

/// Parses the general entity declarations of a DOCTYPE's internal subset.
pub(crate) fn parse_entity_declarations(doctype: &str) -> EntityDeclarations {
    let mut external_parsed: HashSet<String> = HashSet::new();
    let mut raw = HashMap::new();
    // Every general entity *name* declared in the subset, including external
    // (SYSTEM/PUBLIC) and unparsed (NDATA) ones for which we hold no value. A
    // reference to such a name is "declared" and must not be judged undeclared.
    let mut declared: HashSet<String> = HashSet::new();

    // The internal subset lives between '[' and the matching ']'.
    let subset = match (doctype.find('['), doctype.rfind(']')) {
        (Some(start), Some(end)) if start < end => &doctype[start + 1..end],
        _ => return EntityDeclarations::default(),
    };

    let bytes = subset.as_bytes();
    let mut i = 0;
    while let Some(pos) = subset[i..].find("<!ENTITY") {
        let mut j = i + pos + "<!ENTITY".len();

        // Skip whitespace
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // Parameter entity — skip the whole declaration
        if j < bytes.len() && bytes[j] == b'%' {
            i = j;
            continue;
        }
        // Entity name
        let name_start = j;
        while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let name = &subset[name_start..j];
        if !name.is_empty() {
            declared.insert(name.to_string());
        }
        // Skip whitespace
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        // Only quoted internal values; SYSTEM/PUBLIC external entities are skipped.
        if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
            let quote = bytes[j];
            j += 1;
            let value_start = j;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j < bytes.len() && !name.is_empty() {
                raw.entry(name.to_string())
                    .or_insert_with(|| subset[value_start..j].to_string());
            }
        } else if !name.is_empty() {
            // External: SYSTEM/PUBLIC literal(s), then `NDATA name` for an
            // unparsed entity. Find the declaration's end outside quotes.
            let mut k = j;
            let mut quote = None;
            while k < bytes.len() {
                match (quote, bytes[k]) {
                    (Some(q), b) if b == q => quote = None,
                    (None, b'"' | b'\'') => quote = Some(bytes[k]),
                    (None, b'>') => break,
                    _ => {}
                }
                k += 1;
            }
            let unparsed = subset[j..k].split_ascii_whitespace().any(|w| w == "NDATA");
            if !unparsed && !raw.contains_key(name) {
                external_parsed.insert(name.to_string());
            }
        }
        i = j.max(i + pos + 1);
    }

    // Whether declarations after this point might be structurally invisible: a
    // referenced external subset, or a parameter-entity reference in the
    // internal subset (XML 1.0 §5.1). When neither is present, the full set of
    // general entities is visible, so we may reject a reference to an
    // undeclared entity or a reference cycle.
    let external = has_external_subset(doctype);
    let pe_referenced = subset_has_pe_reference(subset);
    let strict = !external && !pe_referenced;

    // The replacement text of an internal entity is its literal value with
    // character references (and parameter-entity references, which are not
    // supported here) expanded; general entity references, including the
    // predefined ones, are left in place and expanded when the entity is
    // used, at which point the replacement text is parsed again
    // (XML 1.0 §4.5, Appendix D).
    let replacements: HashMap<String, String> = raw
        .iter()
        .map(|(name, value)| (name.clone(), replacement_text(value)))
        .collect();

    // In strict mode an entity whose expansion cycles or reaches an
    // undeclared entity is "poisoned": it is omitted from the map so that any
    // *use* of it is reported as an unknown entity (a rejection). Otherwise
    // (external subset or PE in play) every entity is kept.
    if !strict {
        return EntityDeclarations {
            replacements,
            external: external_parsed,
        };
    }
    let mut state: HashMap<&str, Visit> = HashMap::new();
    let names: Vec<&str> = replacements.keys().map(String::as_str).collect();
    let resolvable: HashSet<&str> = names
        .into_iter()
        .filter(|name| resolves(name, &replacements, &declared, &mut state))
        .collect();
    let kept = replacements
        .iter()
        .filter(|(name, _)| resolvable.contains(name.as_str()))
        .map(|(n, v)| (n.clone(), v.clone()))
        .collect();
    EntityDeclarations {
        replacements: kept,
        external: external_parsed,
    }
}

/// True when the part of the DOCTYPE before the internal subset references an
/// external subset (`SYSTEM`/`PUBLIC`).
fn has_external_subset(doctype: &str) -> bool {
    let head = match doctype.find('[') {
        Some(i) => &doctype[..i],
        None => doctype,
    };
    head.contains("SYSTEM") || head.contains("PUBLIC")
}

/// True when the internal subset contains a parameter-entity *reference*
/// (`%name`), as opposed to a parameter-entity *declaration* (`% name`).
fn subset_has_pe_reference(subset: &str) -> bool {
    let mut chars = subset.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(next) = chars.peek()
            && (next.is_alphabetic() || *next == '_' || *next == ':')
        {
            return true;
        }
    }
    false
}

/// Expands the character references in an entity's literal value. A
/// malformed reference is left as written; it is rejected when the entity is
/// used.
fn replacement_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(pos) = rest.find("&#") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let decoded = after.find(';').and_then(|semi| {
            let digits = &after[..semi];
            let code = match digits.strip_prefix('x') {
                Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    u32::from_str_radix(hex, 16).ok()
                }
                None if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
                    digits.parse::<u32>().ok()
                }
                _ => None,
            };
            Some((code.and_then(char::from_u32)?, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Depth-first search state for [`resolves`].
#[derive(Clone, Copy, PartialEq)]
enum Visit {
    InProgress,
    Ok,
    Bad,
}

/// Whether every general entity reachable from `name` is declared and the
/// references form no cycle. Memoized in `state`, so the check is linear in
/// the size of the declarations even for exponential expansions.
fn resolves<'a>(
    name: &'a str,
    replacements: &'a HashMap<String, String>,
    declared: &HashSet<String>,
    state: &mut HashMap<&'a str, Visit>,
) -> bool {
    match state.get(name) {
        Some(Visit::Ok) => return true,
        Some(Visit::Bad) | Some(Visit::InProgress) => return false,
        None => {}
    }
    let Some(text) = replacements.get(name) else {
        // Declared but external/unparsed: we hold no replacement text, so
        // stop here without treating it as undeclared.
        return declared.contains(name);
    };
    state.insert(name, Visit::InProgress);
    let ok = entity_references(text).all(|r| {
        matches!(r, "lt" | "gt" | "amp" | "quot" | "apos")
            || match replacements.get_key_value(r) {
                Some((key, _)) => resolves(key, replacements, declared, state),
                None => declared.contains(r),
            }
    });
    state.insert(name, if ok { Visit::Ok } else { Visit::Bad });
    ok
}

/// The general entity names referenced in a replacement text, skipping
/// character references and the contents of CDATA sections.
fn entity_references(text: &str) -> impl Iterator<Item = &str> {
    let mut names = Vec::new();
    let mut rest = text;
    loop {
        let (chunk, next) = match rest.find("<![CDATA[") {
            Some(cd) => {
                let body = &rest[cd + "<![CDATA[".len()..];
                let next = body.find("]]>").map(|end| &body[end + 3..]);
                (&rest[..cd], next)
            }
            None => (rest, None),
        };
        let mut scan = chunk;
        while let Some(amp) = scan.find('&') {
            let after = &scan[amp + 1..];
            let Some(semi) = after.find(';') else { break };
            let name = &after[..semi];
            if !name.starts_with('#') && !name.is_empty() {
                names.push(name);
            }
            scan = &after[semi + 1..];
        }
        match next {
            Some(n) => rest = n,
            None => break,
        }
    }
    names.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_entities() {
        let map = parse_internal_entities(
            r#"doc [ <!ENTITY internal "text content"> <!ENTITY other 'more'> ]"#,
        );
        assert_eq!(
            map.get("internal").map(String::as_str),
            Some("text content")
        );
        assert_eq!(map.get("other").map(String::as_str), Some("more"));
    }

    #[test]
    fn expands_char_refs_and_nesting() {
        let map = parse_internal_entities(r#"doc [ <!ENTITY a "&#65;"> <!ENTITY b "x&a;y"> ]"#);
        assert_eq!(map.get("a").map(String::as_str), Some("A"));
        // Entity references stay in the replacement text; they are expanded
        // (and the result reparsed) where the entity is used.
        assert_eq!(map.get("b").map(String::as_str), Some("x&a;y"));
    }

    #[test]
    fn predefined_references_stay_in_replacement_text() {
        let map = parse_internal_entities(r#"doc [ <!ENTITY e "a&amp;b&#38;lt;c"> ]"#);
        assert_eq!(map.get("e").map(String::as_str), Some("a&amp;b&lt;c"));
    }

    #[test]
    fn exponential_entities_are_checked_without_expanding() {
        let mut subset = String::from(r#"<!ENTITY a0 "0123456789">"#);
        for i in 1..40 {
            let p = i - 1;
            subset.push_str(&format!(r#"<!ENTITY a{i} "&a{p};&a{p};">"#));
        }
        let map = parse_internal_entities(&format!("doc [ {subset} ]"));
        assert!(map.contains_key("a39"));
    }

    #[test]
    fn skips_parameter_and_external_entities() {
        let map = parse_internal_entities(
            r#"doc [ <!ENTITY % param "x"> <!ENTITY ext SYSTEM "foo.ent"> <!ENTITY ok "v"> ]"#,
        );
        assert!(!map.contains_key("param"));
        assert!(!map.contains_key("ext"));
        assert_eq!(map.get("ok").map(String::as_str), Some("v"));
    }

    #[test]
    fn first_declaration_wins() {
        let map = parse_internal_entities(r#"doc [ <!ENTITY e "one"> <!ENTITY e "two"> ]"#);
        assert_eq!(map.get("e").map(String::as_str), Some("one"));
    }

    #[test]
    fn poisons_reference_cycle() {
        // e1 -> e2 -> e3 -> e1: every entity is omitted so any use is reported
        // as an unknown entity.
        let map = parse_internal_entities(
            r#"doc [ <!ENTITY e1 "&e2;"> <!ENTITY e2 "&e3;"> <!ENTITY e3 "&e1;"> ]"#,
        );
        assert!(map.is_empty(), "cyclic entities must be poisoned: {map:?}");
    }

    #[test]
    fn poisons_reference_to_undeclared_entity() {
        let map = parse_internal_entities(r#"doc [ <!ENTITY foo "&bar;"> ]"#);
        assert!(
            !map.contains_key("foo"),
            "an entity referencing an undeclared entity must be poisoned"
        );
    }

    #[test]
    fn keeps_reference_to_external_entity() {
        // e1 -> e2 -> e3, with e3 an external entity: e3 is declared, so the
        // chain is not undeclared and e1/e2 stay resolvable.
        let map = parse_internal_entities(
            r#"doc [ <!ENTITY e1 "&e2;"> <!ENTITY e2 "&e3;"> <!ENTITY e3 SYSTEM "e.ent"> ]"#,
        );
        assert!(map.contains_key("e1") && map.contains_key("e2"));
    }

    #[test]
    fn cdata_in_entity_value_is_literal() {
        // `&foo;` inside a CDATA section is literal text, not a reference, so
        // `e` is not poisoned even though `foo` is undeclared.
        let map = parse_internal_entities(r#"doc [ <!ENTITY e "<![CDATA[&foo;]]>"> ]"#);
        assert!(map.contains_key("e"));
    }

    #[test]
    fn does_not_poison_when_parameter_entity_referenced() {
        // A PE reference makes declarations potentially invisible: accept
        // (best-effort) rather than poison.
        let map = parse_internal_entities(r#"doc [ <!ENTITY % p "x"> %p; <!ENTITY foo "&bar;"> ]"#);
        assert!(map.contains_key("foo"));
    }
}
