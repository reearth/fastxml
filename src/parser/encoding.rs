//! Input encoding detection and transcoding to UTF-8.
//!
//! The parsers work on UTF-8 internally. This module sniffs the BOM and the
//! XML declaration's `encoding` pseudo-attribute and transcodes non-UTF-8
//! input up front, so the rest of the pipeline never sees other encodings.

use std::borrow::Cow;

use encoding_rs::Encoding;

/// Transcodes `input` to UTF-8 when it is recognizably in another encoding;
/// returns the input unchanged otherwise.
pub(crate) fn to_utf8(input: &[u8]) -> Cow<'_, [u8]> {
    // BOM-based detection (UTF-8 BOM is handled by quick-xml itself).
    if let Some((encoding, _)) = Encoding::for_bom(input) {
        if encoding != encoding_rs::UTF_8 {
            return transcode(encoding, input);
        }
        return Cow::Borrowed(input);
    }

    // BOM-less UTF-16 detection via the mandatory '<' of the prolog.
    if input.len() >= 2 {
        if input[0] == 0x3C && input[1] == 0x00 {
            return transcode(encoding_rs::UTF_16LE, input);
        }
        if input[0] == 0x00 && input[1] == 0x3C {
            return transcode(encoding_rs::UTF_16BE, input);
        }
    }

    // ASCII-compatible prolog: honor the declared encoding.
    if let Some(label) = declared_encoding(input) {
        let lower = label.to_ascii_lowercase();
        if lower != "utf-8" && lower != "us-ascii" && lower != "ascii" {
            if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
                if encoding != encoding_rs::UTF_8 {
                    return transcode(encoding, input);
                }
            }
        }
    }

    Cow::Borrowed(input)
}

/// Extracts the `encoding` pseudo-attribute from an ASCII-compatible XML
/// declaration at the start of the input.
fn declared_encoding(input: &[u8]) -> Option<String> {
    let head = &input[..input.len().min(256)];
    if !head.starts_with(b"<?xml") {
        return None;
    }
    let end = head.windows(2).position(|w| w == b"?>")?;
    let decl = std::str::from_utf8(&head[..end]).ok()?;
    let pos = decl.find("encoding")?;
    let rest = decl[pos + "encoding".len()..].trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &rest[1..];
    let close = value.find(quote)?;
    Some(value[..close].to_string())
}

fn transcode<'a>(encoding: &'static Encoding, input: &'a [u8]) -> Cow<'a, [u8]> {
    let (text, _, _) = encoding.decode(input);
    Cow::Owned(text.into_owned().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_input_is_borrowed() {
        let xml = b"<?xml version=\"1.0\"?><root/>";
        assert!(matches!(to_utf8(xml), Cow::Borrowed(_)));
    }

    #[test]
    fn utf16le_bom_is_transcoded() {
        let text = "<?xml version=\"1.0\"?><root>\u{e9}</root>";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let out = to_utf8(&bytes);
        assert_eq!(std::str::from_utf8(&out).unwrap(), text);
    }

    #[test]
    fn bomless_utf16be_is_transcoded() {
        let text = "<?xml version=\"1.0\"?><root/>";
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        let out = to_utf8(&bytes);
        assert_eq!(std::str::from_utf8(&out).unwrap(), text);
    }

    #[test]
    fn declared_latin1_is_transcoded() {
        let xml = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><root>\xe9</root>";
        let out = to_utf8(xml);
        assert_eq!(
            std::str::from_utf8(&out).unwrap(),
            "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><root>\u{e9}</root>"
        );
    }
}
