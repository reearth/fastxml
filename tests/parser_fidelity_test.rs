//! Data-fidelity regressions for the parser, DOM attribute model, serializer,
//! and XPath attribute tests.
//!
//! Each group pins one way the parsed data used to differ from what a
//! conforming XML processor reports for the same input.

use fastxml::event::XmlEvent;
use fastxml::transform::Transformer;
use fastxml::{Parser, Printer, QueryExt, XmlDocument, parse_schema_locations};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

fn dom(xml: &str) -> XmlDocument {
    Parser::from(xml).parse().unwrap()
}

fn events(xml: &str) -> Vec<XmlEvent> {
    Parser::from(xml).events().unwrap()
}

/// Concatenated text content of the root element, DOM side.
fn dom_text(xml: &str) -> String {
    dom(xml)
        .get_root_element()
        .unwrap()
        .get_content()
        .unwrap_or_default()
}

/// Concatenated text events, streaming side.
fn stream_text(xml: &str) -> String {
    events(xml)
        .into_iter()
        .filter_map(|e| match e {
            XmlEvent::Text(t) | XmlEvent::CData(t) => Some(t),
            _ => None,
        })
        .collect()
}

/// Attribute value of the root element, streaming side, by written QName.
fn stream_root_attr(xml: &str, qname: &str) -> Option<String> {
    events(xml).into_iter().find_map(|e| match e {
        XmlEvent::StartElement { attributes, .. } => attributes
            .iter()
            .find(|(k, _)| k.as_str() == qname)
            .map(|(_, v)| v.to_string()),
        _ => None,
    })
}

fn root_attr(xml: &str, name: &str) -> Option<String> {
    dom(xml).get_root_element().unwrap().get_attribute(name)
}

// =============================================================================
// DOM attribute model: no attribute is lost, prefixes and namespaces are kept
// =============================================================================

mod attribute_model {
    use super::*;

    const COLLIDING: &str = r#"<r xmlns:a="urn:a" xmlns:b="urn:b" a:x="1" b:x="2" x="3"/>"#;

    #[test]
    fn same_local_name_attributes_are_all_kept() {
        let doc = dom(COLLIDING);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_attribute("a:x").as_deref(), Some("1"));
        assert_eq!(root.get_attribute("b:x").as_deref(), Some("2"));
        // A bare local name prefers the attribute that has no prefix.
        assert_eq!(root.get_attribute("x").as_deref(), Some("3"));
        assert_eq!(root.get_attribute_ns("x", "urn:a").as_deref(), Some("1"));
        assert_eq!(root.get_attribute_ns("x", "urn:b").as_deref(), Some("2"));

        let attrs = root.get_attributes();
        let keys: Vec<&str> = attrs.keys().map(String::as_str).collect();
        assert_eq!(keys, ["a:x", "b:x", "x"]);
        assert_eq!(attrs["a:x"], "1");
        assert_eq!(attrs["b:x"], "2");
        assert_eq!(attrs["x"], "3");
    }

    #[test]
    fn unprefixed_attribute_does_not_take_another_attributes_namespace() {
        let doc = dom(r#"<r xmlns:a="urn:a" x="3" a:x="1"/>"#);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_attribute("x").as_deref(), Some("3"));
        assert_eq!(root.get_attribute_ns_info("x"), None);
        assert_eq!(
            root.get_attribute_ns_info("a:x"),
            Some(("a".to_string(), "urn:a".to_string()))
        );
    }

    #[test]
    fn local_name_lookup_still_finds_a_lone_prefixed_attribute() {
        // The common CityGML case: `gml:id` looked up as `id`.
        let doc = dom(r#"<r xmlns:gml="http://www.opengis.net/gml" gml:id="g1"/>"#);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_attribute("id").as_deref(), Some("g1"));
        assert_eq!(root.get_attribute("gml:id").as_deref(), Some("g1"));
        assert_eq!(
            root.get_attributes().get("id").map(String::as_str),
            Some("g1")
        );
    }

    #[test]
    fn get_attribute_ns_checks_the_namespace() {
        let doc = dom(r#"<r xmlns:a="urn:a" a:x="1" y="2"/>"#);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_attribute_ns("x", "urn:a").as_deref(), Some("1"));
        assert_eq!(root.get_attribute_ns("x", "urn:b"), None);
        assert_eq!(root.get_attribute_ns("y", "urn:a"), None);
        // The empty namespace name selects attributes in no namespace.
        assert_eq!(root.get_attribute_ns("y", "").as_deref(), Some("2"));
        assert_eq!(root.get_attribute_ns("x", ""), None);
    }

    #[test]
    fn xml_prefix_is_bound_to_the_xml_namespace() {
        let doc = dom(r#"<r xml:lang="ja"/>"#);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_attribute("xml:lang").as_deref(), Some("ja"));
        assert_eq!(
            root.get_attribute_ns("lang", "http://www.w3.org/XML/1998/namespace")
                .as_deref(),
            Some("ja")
        );
    }

    #[test]
    fn set_attribute_keeps_the_prefix_of_the_attribute_it_replaces() {
        let doc = dom(r#"<r xmlns:gml="http://www.opengis.net/gml" gml:id="g1"/>"#);
        let root = doc.get_root_element().unwrap();
        root.set_attribute("id", "g2");
        assert_eq!(root.get_attribute("gml:id").as_deref(), Some("g2"));
        let out = Printer::from(&root).to_string().unwrap();
        assert!(out.contains(r#"gml:id="g2""#), "{out}");
    }

    #[test]
    fn xsi_schema_location_is_found_by_qualified_name() {
        let xml = format!(r#"<r xmlns:xsi="{XSI}" xsi:schemaLocation="urn:x x.xsd"/>"#);
        assert_eq!(
            root_attr(&xml, "xsi:schemaLocation").as_deref(),
            Some("urn:x x.xsd")
        );
    }
}

// =============================================================================
// schemaLocation discovery is namespace-aware
// =============================================================================

mod schema_locations {
    use super::*;
    use fastxml::parser::parse_schema_locations_from_reader;

    #[test]
    fn schema_location_outside_xsi_namespace_is_ignored() {
        let xml = r#"<r xmlns:foo="urn:foo" foo:schemaLocation="urn:x x.xsd"/>"#;
        assert_eq!(parse_schema_locations(&dom(xml)).unwrap(), vec![]);
        assert_eq!(
            parse_schema_locations_from_reader(xml.as_bytes()).unwrap(),
            vec![]
        );
    }

    #[test]
    fn schema_location_is_found_under_any_prefix_bound_to_xsi() {
        let xml = format!(r#"<r xmlns:s="{XSI}" s:schemaLocation="urn:x x.xsd"/>"#);
        let expected = vec![("urn:x".to_string(), "x.xsd".to_string())];
        assert_eq!(parse_schema_locations(&dom(&xml)).unwrap(), expected);
        assert_eq!(
            parse_schema_locations_from_reader(xml.as_bytes()).unwrap(),
            expected
        );
    }

    #[test]
    fn xsi_schema_location_wins_over_a_colliding_attribute() {
        let xml = format!(
            r#"<r xmlns:xsi="{XSI}" xmlns:foo="urn:foo" foo:schemaLocation="urn:y y.xsd" xsi:schemaLocation="urn:x x.xsd"/>"#
        );
        let expected = vec![("urn:x".to_string(), "x.xsd".to_string())];
        assert_eq!(parse_schema_locations(&dom(&xml)).unwrap(), expected);
        assert_eq!(
            parse_schema_locations_from_reader(xml.as_bytes()).unwrap(),
            expected
        );
    }
}

// =============================================================================
// Serializer keeps prefixes and escapes white space so output round-trips
// =============================================================================

mod serializer {
    use super::*;

    #[test]
    fn printer_writes_xml_lang_with_its_prefix() {
        let doc = dom(r#"<r xml:lang="ja"/>"#);
        let out = Printer::from(&doc.get_root_element().unwrap())
            .to_string()
            .unwrap();
        assert_eq!(out, r#"<r xml:lang="ja"/>"#);
    }

    #[test]
    fn printer_writes_every_colliding_attribute() {
        let doc = dom(super::attribute_model_colliding());
        let out = Printer::from(&doc.get_root_element().unwrap())
            .to_string()
            .unwrap();
        assert!(out.contains(r#"a:x="1""#), "{out}");
        assert!(out.contains(r#"b:x="2""#), "{out}");
        assert!(out.contains(r#" x="3""#), "{out}");
    }

    #[test]
    fn attribute_whitespace_and_text_cr_are_escaped() {
        let doc = dom(r#"<r a="x&#9;y&#10;z&#13;w">a&#13;b</r>"#);
        let out = Printer::from(&doc.get_root_element().unwrap())
            .to_string()
            .unwrap();
        assert_eq!(out, r#"<r a="x&#9;y&#10;z&#13;w">a&#13;b</r>"#);
    }

    #[test]
    fn serialized_output_round_trips() {
        let doc = dom(r#"<r a="x&#9;y&#10;z&#13;w">a&#13;b&#10;c</r>"#);
        let out = Printer::from(&doc.get_root_element().unwrap())
            .to_string()
            .unwrap();
        let again = dom(&out);
        let root = again.get_root_element().unwrap();
        assert_eq!(root.get_attribute("a").as_deref(), Some("x\ty\nz\rw"));
        assert_eq!(root.get_content().as_deref(), Some("a\rb\nc"));
    }

    #[test]
    fn set_attribute_value_with_whitespace_round_trips() {
        let doc = dom("<r/>");
        let root = doc.get_root_element().unwrap();
        root.set_attribute("v", "a\tb\nc\"<&");
        let out = Printer::from(&root).to_string().unwrap();
        assert_eq!(
            dom(&out)
                .get_root_element()
                .unwrap()
                .get_attribute("v")
                .as_deref(),
            Some("a\tb\nc\"<&")
        );
    }
}

fn attribute_model_colliding() -> &'static str {
    r#"<r xmlns:a="urn:a" xmlns:b="urn:b" a:x="1" b:x="2" x="3"/>"#
}

// =============================================================================
// XPath attribute name tests and lang()
// =============================================================================

mod xpath_attributes {
    use super::*;

    const DOC: &str =
        r#"<root xmlns:p="urn:p"><a id="x1" p:attr="v"/><b lang="fr"><c/></b></root>"#;

    fn count(xml: &str, xpath: &str) -> usize {
        dom(xml).query_nodes(xpath).unwrap().len()
    }

    #[test]
    fn prefixed_attribute_test_requires_the_namespace() {
        assert_eq!(count(DOC, "//a[@p:id]"), 0);
        assert_eq!(count(DOC, "//a[@p:attr]"), 1);
        assert_eq!(count(DOC, "//a/@p:*"), 1);
    }

    #[test]
    fn unbound_prefix_in_attribute_test_is_an_error() {
        let doc = dom(DOC);
        assert!(doc.query("//a/@q:attr").is_err());
        assert!(doc.query("//a/@q:id").is_err());
    }

    #[test]
    fn xml_lang_test_does_not_match_a_plain_lang_attribute() {
        assert_eq!(count(DOC, "//b[@xml:lang]"), 0);
        assert_eq!(count(DOC, "//b/@xml:lang"), 0);
    }

    #[test]
    fn colliding_attributes_are_selected_by_namespace() {
        let xml = attribute_model_colliding();
        let doc = dom(xml);
        let r = doc.query("string(/r/@a:x)");
        // `a` is declared on the root element, so it is bound for XPath.
        assert_eq!(r.unwrap().to_string_value(), "1");
        assert_eq!(doc.query("string(/r/@b:x)").unwrap().to_string_value(), "2");
        assert_eq!(doc.query("string(/r/@x)").unwrap().to_string_value(), "3");
        assert_eq!(doc.query_nodes("/r/@*").unwrap().len(), 3);
    }

    #[test]
    fn attribute_name_includes_xml_prefix() {
        let doc = dom(r#"<root xml:lang="en"/>"#);
        assert_eq!(
            doc.query("name(/root/@*)").unwrap().to_string_value(),
            "xml:lang"
        );
    }

    #[test]
    fn lang_uses_only_xml_lang() {
        let xml = r#"<root xml:lang="en-US"><b lang="fr"><c/></b></root>"#;
        assert_eq!(count(xml, "//c[lang('fr')]"), 0);
        assert_eq!(count(xml, "//c[lang('en')]"), 1);
    }
}

// =============================================================================
// Transform keeps attribute prefixes inside matched elements
// =============================================================================

mod transform_attributes {
    use super::*;

    const DOC: &str = r##"<root xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:gml="http://www.opengis.net/gml"><item xml:lang="ja" xlink:href="#a" gml:id="g1"/></root>"##;
    const COLLIDING: &str =
        r#"<root xmlns:a="urn:a" xmlns:b="urn:b"><item a:id="1" b:id="2"/></root>"#;

    fn assert_prefixes_kept(out: &str) {
        assert!(out.contains(r#"xml:lang="ja""#), "{out}");
        assert!(out.contains(r##"xlink:href="#a""##), "{out}");
        assert!(out.contains(r#"gml:id="g1""#), "{out}");
    }

    #[test]
    fn streaming_rebuild_keeps_unregistered_prefixes() {
        let out = Transformer::from(DOC)
            .on("//item", |_| {})
            .to_string()
            .unwrap();
        assert_prefixes_kept(&out);
    }

    #[test]
    fn streaming_rebuild_keeps_colliding_attributes() {
        let out = Transformer::from(COLLIDING)
            .on("//item", |_| {})
            .to_string()
            .unwrap();
        assert!(out.contains(r#"a:id="1""#), "{out}");
        assert!(out.contains(r#"b:id="2""#), "{out}");
    }

    #[test]
    fn fallback_rebuild_keeps_prefixes_and_colliding_attributes() {
        // `last()` is not streamable, so this runs on the DOM fallback path.
        let out = Transformer::from(DOC)
            .allow_fallback()
            .on("//item[last()]", |_| {})
            .to_string()
            .unwrap();
        assert_prefixes_kept(&out);
        let out = Transformer::from(COLLIDING)
            .allow_fallback()
            .on("//item[last()]", |_| {})
            .to_string()
            .unwrap();
        assert!(out.contains(r#"a:id="1""#), "{out}");
        assert!(out.contains(r#"b:id="2""#), "{out}");
    }
}

// =============================================================================
// Internal DTD entities: replacement text is rescanned
// =============================================================================

mod entities {
    use super::*;

    #[test]
    fn predefined_reference_inside_entity_value_survives() {
        let xml = r#"<!DOCTYPE r [<!ENTITY e "a&amp;b">]><r v="&e;">&e;</r>"#;
        assert_eq!(dom_text(xml), "a&b");
        assert_eq!(stream_text(xml), "a&b");
        assert_eq!(root_attr(xml, "v").as_deref(), Some("a&b"));
        assert_eq!(stream_root_attr(xml, "v").as_deref(), Some("a&b"));
    }

    #[test]
    fn escaped_reference_is_expanded_when_the_entity_is_used() {
        // `&#38;` becomes `&` at declaration time, so the replacement text is
        // `a&lt;b`, which reads as `a<b` when the entity is referenced.
        let xml = r#"<!DOCTYPE r [<!ENTITY e "a&#38;lt;b">]><r v="&e;">&e;</r>"#;
        assert_eq!(dom_text(xml), "a<b");
        assert_eq!(stream_text(xml), "a<b");
        assert_eq!(root_attr(xml, "v").as_deref(), Some("a<b"));
        assert_eq!(stream_root_attr(xml, "v").as_deref(), Some("a<b"));
    }

    #[test]
    fn markup_in_entity_becomes_elements() {
        let xml = r#"<!DOCTYPE r [<!ENTITY e "<b>x</b>">]><r>1&e;2</r>"#;
        let doc = dom(xml);
        let root = doc.get_root_element().unwrap();
        let kids = root.get_child_elements();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].get_name(), "b");
        assert_eq!(kids[0].get_content().as_deref(), Some("x"));
        assert_eq!(root.get_content().as_deref(), Some("1x2"));

        let names: Vec<String> = events(xml)
            .into_iter()
            .filter_map(|e| match e {
                XmlEvent::StartElement { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["r", "b"]);
        assert_eq!(stream_text(xml), "1x2");
    }

    #[test]
    fn markup_reached_through_a_nested_entity_becomes_elements() {
        let xml = r#"<!DOCTYPE r [<!ENTITY i "<i/>"><!ENTITY e "x&i;y">]><r>&e;</r>"#;
        let doc = dom(xml);
        let root = doc.get_root_element().unwrap();
        assert_eq!(root.get_child_elements()[0].get_name(), "i");
        assert_eq!(root.get_content().as_deref(), Some("xy"));
        assert_eq!(stream_text(xml), "xy");
    }

    #[test]
    fn char_ref_markup_in_entity_becomes_elements() {
        // XML 1.0 Appendix D: `&#60;` is expanded at declaration time, so the
        // replacement text contains a real start tag.
        let xml = r#"<!DOCTYPE r [<!ENTITY e "&#60;b/>">]><r>&e;</r>"#;
        let doc = dom(xml);
        assert_eq!(
            doc.get_root_element().unwrap().get_child_elements()[0].get_name(),
            "b"
        );
    }

    #[test]
    fn lt_reaching_an_attribute_through_an_entity_is_rejected() {
        let xml = r#"<!DOCTYPE r [<!ENTITY e "&#60;">]><r a="&e;"/>"#;
        assert!(Parser::from(xml).parse().is_err());
        assert!(Parser::from(xml).events().is_err());
    }

    #[test]
    fn unbalanced_markup_in_entity_is_rejected() {
        let xml = r#"<!DOCTYPE r [<!ENTITY e "<b>">]><r>&e;</r>"#;
        assert!(Parser::from(xml).parse().is_err());
        assert!(Parser::from(xml).events().is_err());
        let xml = r#"<!DOCTYPE r [<!ENTITY e "</r><r>">]><r>&e;</r>"#;
        assert!(Parser::from(xml).parse().is_err());
        assert!(Parser::from(xml).events().is_err());
    }

    #[test]
    fn whitespace_from_an_entity_in_an_attribute_is_normalized() {
        let xml = "<!DOCTYPE r [<!ENTITY e \"a&#9;b\">]><r v=\"&e;\">&e;</r>";
        assert_eq!(root_attr(xml, "v").as_deref(), Some("a b"));
        assert_eq!(stream_root_attr(xml, "v").as_deref(), Some("a b"));
        assert_eq!(dom_text(xml), "a\tb");
    }

    #[test]
    fn reference_to_unloaded_external_entity_is_an_error_not_silent_loss() {
        // External entities are not loaded; their content must not vanish
        // silently, whether referenced directly or through another entity.
        for xml in [
            r#"<!DOCTYPE r [<!ENTITY x SYSTEM "x.ent">]><r>&x;</r>"#,
            r#"<!DOCTYPE r [<!ENTITY x SYSTEM "x.ent"><!ENTITY e "a&x;">]><r>&e;</r>"#,
        ] {
            assert!(Parser::from(xml).parse().is_err(), "{xml}");
            assert!(Parser::from(xml).events().is_err(), "{xml}");
        }
    }

    #[test]
    fn hex_character_reference_needs_lowercase_x() {
        // CharRef ::= '&#x' [0-9a-fA-F]+ ';' — `&#X41;` is not a reference.
        for xml in [
            "<r>&#X41;</r>",
            r#"<r a="&#X41;"/>"#,
            r#"<!DOCTYPE r [<!ENTITY e "&#X41;">]><r>&e;</r>"#,
        ] {
            assert!(Parser::from(xml).parse().is_err(), "{xml}");
            assert!(Parser::from(xml).events().is_err(), "{xml}");
        }
        assert_eq!(dom_text("<r>&#x41;&#65;</r>"), "AA");
    }

    #[test]
    fn unterminated_doctype_is_rejected() {
        let xml = "<!DOCTYPE r [<!ENTITY e \"x>\n]>\n<r>&e;</r>";
        assert!(Parser::from(xml).parse().is_err());
        assert!(Parser::from(xml).events().is_err());
    }

    #[test]
    fn exponential_entity_expansion_is_rejected() {
        let mut dtd = String::from(r#"<!ENTITY a0 "0123456789">"#);
        for i in 1..12 {
            let p = i - 1;
            dtd.push_str(&format!(
                r#"<!ENTITY a{i} "&a{p};&a{p};&a{p};&a{p};&a{p};&a{p};&a{p};&a{p};&a{p};&a{p};">"#
            ));
        }
        let xml = format!("<!DOCTYPE r [{dtd}]><r>&a11;</r>");
        assert!(Parser::from(xml.as_str()).parse().is_err());
        assert!(Parser::from(xml.as_str()).events().is_err());
    }
}

// =============================================================================
// End-of-line (§2.11) and attribute-value (§3.3.3) normalization
// =============================================================================

mod normalization {
    use super::*;

    #[test]
    fn line_breaks_in_text_become_lf() {
        let xml = "<r>a\r\nb\rc\r</r>";
        assert_eq!(dom_text(xml), "a\nb\nc\n");
        assert_eq!(stream_text(xml), "a\nb\nc\n");
    }

    #[test]
    fn line_breaks_in_cdata_comments_and_reader_input_become_lf() {
        let xml = "<r><![CDATA[a\r\nb]]><!--c\r\nd--></r>";
        assert_eq!(dom_text(xml), "a\nb");
        assert_eq!(stream_text(xml), "a\nb");
        let comments: Vec<String> = events(xml)
            .into_iter()
            .filter_map(|e| match e {
                XmlEvent::Comment(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(comments, ["c\nd"]);

        // The same through the BufRead entry point, with a CRLF split across
        // the reader's internal buffer boundary.
        let input = format!("<r>{}\r\n{}</r>", "x".repeat(8187), "y");
        let reader = std::io::BufReader::with_capacity(8192, input.as_bytes());
        let doc = Parser::from_reader(reader).parse().unwrap();
        let text = doc.get_root_element().unwrap().get_content().unwrap();
        assert!(text.ends_with("x\ny"), "{:?}", &text[text.len() - 4..]);
        assert!(!text.contains('\r'));
    }

    #[test]
    fn literal_whitespace_in_attribute_values_becomes_space() {
        let xml = "<r a=\"x\ty\nz\r\nw\"/>";
        assert_eq!(root_attr(xml, "a").as_deref(), Some("x y z w"));
        assert_eq!(stream_root_attr(xml, "a").as_deref(), Some("x y z w"));
    }

    #[test]
    fn character_references_in_attribute_values_are_kept() {
        let xml = r#"<r a="x&#9;y&#10;z&#13;w"/>"#;
        assert_eq!(root_attr(xml, "a").as_deref(), Some("x\ty\nz\rw"));
        assert_eq!(stream_root_attr(xml, "a").as_deref(), Some("x\ty\nz\rw"));
    }

    #[test]
    fn character_reference_cr_in_text_is_kept() {
        let xml = "<r>a&#13;b&#13;&#10;c</r>";
        assert_eq!(dom_text(xml), "a\rb\r\nc");
        assert_eq!(stream_text(xml), "a\rb\r\nc");
    }

    #[test]
    fn line_numbers_count_every_kind_of_line_break() {
        let xml = "<r>\r<a/>\r\n<b/>\n<c/></r>";
        let doc = dom(xml);
        let lines: Vec<Option<usize>> = doc
            .get_root_element()
            .unwrap()
            .get_child_elements()
            .iter()
            .map(|n| n.line())
            .collect();
        assert_eq!(lines, [Some(2), Some(3), Some(4)]);
        let stream_lines: Vec<Option<usize>> = events(xml)
            .into_iter()
            .filter_map(|e| match e {
                XmlEvent::StartElement { name, line, .. } if &*name != "r" => Some(line),
                _ => None,
            })
            .collect();
        assert_eq!(stream_lines, lines);
    }
}
