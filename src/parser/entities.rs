//! Internal DTD subset parsing for general entity declarations.
//!
//! quick-xml hands the DOCTYPE declaration over as raw text; this module
//! extracts `<!ENTITY name "value">` declarations from the internal subset
//! so entity references in content and attribute values can be resolved.
//! External (SYSTEM/PUBLIC) and parameter (`%`) entities are skipped.

use std::collections::HashMap;

/// Parses the internal DTD subset of a DOCTYPE declaration into a map of
/// general entity name → fully expanded replacement text.
pub(crate) fn parse_internal_entities(doctype: &str) -> HashMap<String, String> {
    let mut raw = HashMap::new();

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

    // Pre-expand each value (character references and nested general
    // entities), since quick-xml inserts resolver replacements literally
    // without rescanning them.
    raw.keys()
        .map(|name| (name.clone(), expand(&raw[name], &raw, 0)))
        .collect()
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
}
