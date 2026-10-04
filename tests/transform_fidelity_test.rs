//! Output fidelity of the transform engines: byte-order marks, processing
//! instructions inside matched elements, and the context passed to
//! `on_with_context` under fallback.

use fastxml::transform::{EditableNode, Transformer};

fn mark(n: &mut EditableNode) {
    n.set_attribute("m", "1");
}

fn reader_transform(xml: &str, xpath: &str) -> String {
    let mut out = Vec::new();
    Transformer::from_reader(std::io::Cursor::new(xml.as_bytes()))
        .on(xpath, mark)
        .write_to(&mut out)
        .unwrap();
    String::from_utf8(out).unwrap()
}

// =============================================================================
// UTF-8 byte-order mark
// =============================================================================

mod bom {
    use super::*;

    const XML: &str =
        "\u{feff}<?xml version=\"1.0\"?>\n<root><item id=\"1\"/><item id=\"2\">t</item></root>";
    const MARKED: &str = "\u{feff}<?xml version=\"1.0\"?>\n<root><item id=\"1\" m=\"1\"/><item id=\"2\" m=\"1\">t</item></root>";

    #[test]
    fn test_single_handler_keeps_offsets_and_bom() {
        let out = Transformer::from(XML)
            .on("//item", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_single_handler_with_context() {
        let out = Transformer::from(XML)
            .on_with_context("//item", |n, _| mark(n))
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_remove() {
        let out = Transformer::from(XML)
            .on("//item", |n| n.remove())
            .to_string()
            .unwrap();
        assert_eq!(out, "\u{feff}<?xml version=\"1.0\"?>\n<root></root>");
    }

    #[test]
    fn test_multi_handler() {
        let out = Transformer::from(XML)
            .on("//item", mark)
            .on("//other", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);

        let out = Transformer::from(XML)
            .on_with_context("//item", |n, _| mark(n))
            .on("//other", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_fallback() {
        let out = Transformer::from(XML)
            .allow_fallback()
            .on("//item[last()]", mark)
            .to_string()
            .unwrap();
        assert_eq!(
            out,
            "\u{feff}<?xml version=\"1.0\"?>\n<root><item id=\"1\"/><item id=\"2\" m=\"1\">t</item></root>"
        );
    }

    #[test]
    fn test_for_each_and_collect() {
        let ids: Vec<String> = Transformer::from(XML)
            .collect("//item", |n| n.get_attribute("id").unwrap_or_default())
            .unwrap();
        assert_eq!(ids, vec!["1", "2"]);
    }

    #[test]
    fn test_reader_keeps_bom() {
        assert_eq!(reader_transform(XML, "//item"), MARKED);
    }
}

// =============================================================================
// Processing instructions inside a matched subtree
// =============================================================================

mod processing_instructions {
    use super::*;

    const XML: &str = "<root><item id=\"1\">a<?pi data?><!--c--><?empty?></item></root>";
    const MARKED: &str = "<root><item id=\"1\" m=\"1\">a<?pi data?><!--c--><?empty?></item></root>";

    #[test]
    fn test_single_handler() {
        let out = Transformer::from(XML)
            .on("//item", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_single_handler_with_context() {
        let out = Transformer::from(XML)
            .on_with_context("//item", |n, _| mark(n))
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_multi_handler() {
        let out = Transformer::from(XML)
            .on("//item", mark)
            .on("//other", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_fallback() {
        let out = Transformer::from(XML)
            .allow_fallback()
            .on("//item[last()]", mark)
            .to_string()
            .unwrap();
        assert_eq!(out, MARKED);
    }

    #[test]
    fn test_reader() {
        assert_eq!(reader_transform(XML, "//item"), MARKED);
        let mut out = Vec::new();
        Transformer::from_reader(std::io::Cursor::new(XML.as_bytes()))
            .on("//item", mark)
            .on("//other", mark)
            .write_to(&mut out)
            .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), MARKED);
    }

    #[test]
    fn test_for_each_sees_pi() {
        let mut xml_out = String::new();
        Transformer::from(XML)
            .on("//item", |n| xml_out = n.to_xml().unwrap())
            .for_each()
            .unwrap();
        assert_eq!(
            xml_out,
            "<item id=\"1\">a<?pi data?><!--c--><?empty?></item>"
        );
    }
}

// =============================================================================
// on_with_context under fallback
// =============================================================================

mod fallback_context {
    use super::*;

    const XML: &str =
        r#"<root><items id="l1"><item/></items><items id="l2"><item/><item/></items></root>"#;

    type Seen = Vec<(usize, usize, String, Option<String>)>;

    fn record(seen: &mut Seen, ctx: &fastxml::transform::TransformContext) {
        seen.push((
            ctx.depth(),
            ctx.position(),
            ctx.path_id(),
            ctx.parent_attribute("id").cloned(),
        ));
    }

    #[test]
    fn test_transform_context_matches_streaming() {
        let mut streaming = Vec::new();
        Transformer::from(XML)
            .on_with_context("//items[2]/item", |_, ctx| record(&mut streaming, ctx))
            .to_string()
            .unwrap();

        let mut fallback = Vec::new();
        Transformer::from(XML)
            .allow_fallback()
            .on_with_context("//items[last()]/item", |_, ctx| record(&mut fallback, ctx))
            .to_string()
            .unwrap();

        assert_eq!(
            fallback,
            vec![
                (3, 1, "root/items[2]".to_string(), Some("l2".to_string())),
                (3, 2, "root/items[2]".to_string(), Some("l2".to_string())),
            ]
        );
        assert_eq!(fallback, streaming);
    }

    #[test]
    fn test_for_each_context() {
        let mut fallback = Vec::new();
        Transformer::from(XML)
            .allow_fallback()
            .on_with_context("//item[last()]", |_, ctx| record(&mut fallback, ctx))
            .for_each()
            .unwrap();
        assert_eq!(
            fallback,
            vec![
                (3, 1, "root/items".to_string(), Some("l1".to_string())),
                (3, 2, "root/items[2]".to_string(), Some("l2".to_string())),
            ]
        );
    }
}
