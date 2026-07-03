//! Internal DTD subset parsing for general entity declarations.
//!
//! quick-xml hands the DOCTYPE declaration over as raw text; this module
//! extracts `<!ENTITY name "value">` declarations from the internal subset
//! so entity references in content and attribute values can be resolved.
//! External (SYSTEM/PUBLIC) and parameter (`%`) entities are skipped.

use std::collections::{HashMap, HashSet};

/// Parses the internal DTD subset of a DOCTYPE declaration into a map of
/// general entity name → fully expanded replacement text.
pub(crate) fn parse_internal_entities(doctype: &str) -> HashMap<String, String> {
    let mut raw = HashMap::new();
    // Every general entity *name* declared in the subset, including external
    // (SYSTEM/PUBLIC) and unparsed (NDATA) ones for which we hold no value. A
    // reference to such a name is "declared" and must not be judged undeclared.
    let mut declared: HashSet<String> = HashSet::new();

    // The internal subset lives between '[' and the matching ']'.
    let subset = match (doctype.find('['), doctype.rfind(']')) {
        (Some(start), Some(end)) if start < end => &doctype[start + 1..end],
        _ => return raw,
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

    // Pre-expand each value (character references and nested general
    // entities), since quick-xml inserts resolver replacements literally
    // without rescanning them. In strict mode an entity whose expansion cycles
    // or reaches an undeclared entity is "poisoned": it is omitted from the map
    // so that any *use* of it is reported as an unknown entity (a rejection).
    let mut out = HashMap::new();
    for name in raw.keys() {
        let mut path = vec![name.clone()];
        match expand_checked(&raw[name], &raw, &declared, &mut path) {
            Ok(value) => {
                out.insert(name.clone(), value);
            }
            Err(_) if strict => { /* poison: leave undeclared */ }
            Err(_) => {
                // Ambiguous (external subset or PE in play): keep a best-effort
                // expansion and accept.
                out.insert(name.clone(), expand(&raw[name], &raw, 0));
            }
        }
    }
    out
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

/// Fully expands an entity value, failing on a reference cycle or a reference
/// to an entity that is neither predefined nor internally declared.
fn expand_checked(
    value: &str,
    entities: &HashMap<String, String>,
    declared: &HashSet<String>,
    path: &mut Vec<String>,
) -> Result<String, ExpandError> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    loop {
        // A CDATA section inside an entity value is literal text: any `&name;`
        // it contains is not a reference.
        if let Some(cd) = rest.find("<![CDATA[") {
            out.push_str(&scan_refs(&rest[..cd], entities, declared, path)?);
            let body = &rest[cd + "<![CDATA[".len()..];
            match body.find("]]>") {
                Some(end) => {
                    out.push_str(&body[..end]);
                    rest = &body[end + 3..];
                    continue;
                }
                None => {
                    out.push_str(body);
                    return Ok(out);
                }
            }
        }
        out.push_str(&scan_refs(rest, entities, declared, path)?);
        return Ok(out);
    }
}

/// Expands references in a fragment that contains no CDATA section.
fn scan_refs(
    value: &str,
    entities: &HashMap<String, String>,
    declared: &HashSet<String>,
    path: &mut Vec<String>,
) -> Result<String, ExpandError> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';') else {
            // A bare '&' is handled by character-level checks elsewhere.
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..semi];
        if let Some(stripped) = name.strip_prefix('#') {
            let code = if let Some(hex) = stripped.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                stripped.parse::<u32>().ok()
            };
            if let Some(c) = code.and_then(char::from_u32) {
                out.push(c);
            }
        } else if matches!(name, "lt" | "gt" | "amp" | "quot" | "apos") {
            // Predefined entities are always available.
        } else if let Some(replacement) = entities.get(name) {
            if path.iter().any(|p| p == name) {
                return Err(ExpandError::Cycle);
            }
            path.push(name.to_string());
            let expanded = expand_checked(replacement, entities, declared, path)?;
            path.pop();
            out.push_str(&expanded);
        } else if declared.contains(name) {
            // Declared but external/unparsed: we hold no replacement text, so
            // stop expanding here without treating it as undeclared.
        } else {
            return Err(ExpandError::Undeclared);
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Why a strict entity expansion failed.
enum ExpandError {
    /// A reference cycle among internal entities.
    Cycle,
    /// A reference to an entity that is neither predefined nor internally
    /// declared.
    Undeclared,
}

/// Expands character references and known general entity references in an
/// entity value. Depth-limited to break reference cycles.
fn expand(value: &str, entities: &HashMap<String, String>, depth: usize) -> String {
    if depth > 8 {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';') else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..semi];
        if let Some(stripped) = name.strip_prefix('#') {
            let code = if let Some(hex) = stripped.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                stripped.parse::<u32>().ok()
            };
            match code.and_then(char::from_u32) {
                Some(c) => out.push(c),
                None => {
                    out.push('&');
                    out.push_str(&after[..=semi]);
                }
            }
        } else if let Some(replacement) = entities.get(name) {
            out.push_str(&expand(replacement, entities, depth + 1));
        } else {
            match name {
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "amp" => out.push('&'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ => {
                    out.push('&');
                    out.push_str(&after[..=semi]);
                }
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
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
        assert_eq!(map.get("b").map(String::as_str), Some("xAy"));
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
