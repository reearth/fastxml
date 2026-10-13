//! Tests for read-only match handlers (`Transformer::on_match`, #49) and
//! early stopping (`ControlFlow::Break`, `.limit(n)`, #50).

use std::io::BufReader;
use std::ops::ControlFlow;

use fastxml::transform::{ElementMatch, TransformError, Transformer};

const NS: &str = "urn:example:b";

const XML: &str = r#"<root xmlns:b="urn:example:b">
  <b:item id="1" b:kind="x" lang="en"><b:item id="2"/><other/></b:item>
  <b:item id="3">text</b:item>
  <other id="4"/>
</root>"#;

/// Runs `f` against an in-memory and a reader-backed transformer over `xml`.
fn both<'x>(xml: &'x str, f: impl Fn(&str, Transformer<'x>)) {
    f("in-memory", Transformer::from(xml));
    f(
        "reader",
        Transformer::from_reader(BufReader::new(xml.as_bytes())),
    );
}

/// Owned snapshot of a match, for assertions.
#[derive(Debug, PartialEq)]
struct Seen {
    name: String,
    qname: String,
    prefix: Option<String>,
    namespace_uri: Option<String>,
    attributes: Vec<(String, String)>,
}

impl Seen {
    fn of(m: &ElementMatch) -> Self {
        Seen {
            name: m.name().to_string(),
            qname: m.qname().to_string(),
            prefix: m.prefix().map(str::to_string),
            namespace_uri: m.namespace_uri().map(str::to_string),
            attributes: m
                .get_attributes()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// on_match (#49)
// ---------------------------------------------------------------------------

#[test]
fn on_match_reports_name_namespace_and_attributes_in_document_order() {
    both(XML, |engine, t| {
        let mut seen = Vec::new();
        t.namespace("b", NS)
            .on_match("//b:item[@id='1']", |m| {
                seen.push(Seen::of(m));
                ControlFlow::Continue(())
            })
            .for_each()
            .unwrap();
        assert_eq!(
            seen,
            [Seen {
                name: "item".into(),
                qname: "b:item".into(),
                prefix: Some("b".into()),
                namespace_uri: Some(NS.into()),
                attributes: vec![
                    ("id".into(), "1".into()),
                    ("b:kind".into(), "x".into()),
                    ("lang".into(), "en".into()),
                ],
            }],
            "{engine}"
        );
    });
}

#[test]
fn on_match_attribute_lookup_by_name_and_namespace() {
    both(XML, |engine, t| {
        let mut found = Vec::new();
        t.namespace("b", NS)
            .on_match("//b:item[@id='1']", |m| {
                found.push((
                    m.get_attribute("lang").map(str::to_string),
                    m.get_attribute("b:kind").map(str::to_string),
                    m.get_attribute_ns(NS, "kind").map(str::to_string),
                    m.get_attribute("missing").map(str::to_string),
                ));
                ControlFlow::Continue(())
            })
            .for_each()
            .unwrap();
        assert_eq!(
            found,
            [(Some("en".into()), Some("x".into()), Some("x".into()), None)],
            "{engine}"
        );
    });
}

#[test]
fn on_match_reports_nested_matches_unlike_on() {
    both(XML, |engine, t| {
        let mut ids = Vec::new();
        t.namespace("b", NS)
            .on_match("//b:item", |m| {
                ids.push(m.get_attribute("id").unwrap().to_string());
                ControlFlow::Continue(())
            })
            .for_each()
            .unwrap();
        assert_eq!(ids, ["1", "2", "3"], "{engine}");
    });
    // `on` keeps skipping a match nested inside another one.
    both(XML, |engine, t| {
        let mut ids = Vec::new();
        t.namespace("b", NS)
            .on("//b:item", |node| {
                ids.push(node.get_attribute("id").unwrap())
            })
            .for_each()
            .unwrap();
        assert_eq!(ids, ["1", "3"], "{engine}");
    });
}

#[test]
fn on_match_sees_matches_inside_an_on_subtree() {
    both(XML, |engine, t| {
        let mut on_ids = Vec::new();
        let mut match_ids = Vec::new();
        t.namespace("b", NS)
            .on("//b:item[@id='1']", |node| {
                on_ids.push(node.get_attribute("id").unwrap())
            })
            .on_match("//b:item", |m| {
                match_ids.push(m.get_attribute("id").unwrap().to_string());
                ControlFlow::Continue(())
            })
            .for_each()
            .unwrap();
        assert_eq!(on_ids, ["1"], "{engine}");
        assert_eq!(match_ids, ["1", "2", "3"], "{engine}");
    });
}

#[test]
fn on_match_break_stops_that_handler() {
    both(XML, |engine, t| {
        let mut ids = Vec::new();
        t.namespace("b", NS)
            .on_match("//b:item", |m| {
                ids.push(m.get_attribute("id").unwrap().to_string());
                if ids.len() == 2 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .for_each()
            .unwrap();
        assert_eq!(ids, ["1", "2"], "{engine}");
    });
}

#[test]
fn on_match_requires_a_streamable_xpath_when_streaming() {
    let result = Transformer::from_reader(BufReader::new(XML.as_bytes()))
        .on_match("//other[last()]", |_| ControlFlow::Continue(()))
        .for_each();
    assert!(
        matches!(result, Err(TransformError::NotStreamable { .. })),
        "{result:?}"
    );
}

#[test]
fn on_match_uses_the_dom_under_fallback() {
    let mut seen = Vec::new();
    Transformer::from(XML)
        .namespace("b", NS)
        .allow_fallback()
        .on_match("//b:item[last()]", |m| {
            seen.push(Seen::of(m));
            ControlFlow::Continue(())
        })
        .for_each()
        .unwrap();
    // `last()` is evaluated per parent: the nested item and the second
    // top-level item. Matches come in document order.
    let ids: Vec<_> = seen
        .iter()
        .map(|s| {
            s.attributes
                .iter()
                .find(|(k, _)| k == "id")
                .unwrap()
                .1
                .clone()
        })
        .collect();
    assert_eq!(ids, ["2", "3"]);
    assert_eq!(seen[0].namespace_uri.as_deref(), Some(NS));
    assert_eq!(seen[0].qname, "b:item");
}

// ---------------------------------------------------------------------------
// limit (#50)
// ---------------------------------------------------------------------------

#[test]
fn limit_caps_on_callbacks() {
    let xml = "<r><i n='1'/><i n='2'/><i n='3'/></r>";
    both(xml, |engine, t| {
        let mut seen = Vec::new();
        t.on("//i", |node| seen.push(node.get_attribute("n").unwrap()))
            .limit(2)
            .for_each()
            .unwrap();
        assert_eq!(seen, ["1", "2"], "{engine}");
    });
}

#[test]
fn limit_caps_on_match_callbacks() {
    let xml = "<r><i n='1'/><i n='2'/><i n='3'/></r>";
    both(xml, |engine, t| {
        let mut seen = Vec::new();
        t.on_match("//i", |m| {
            seen.push(m.get_attribute("n").unwrap().to_string());
            ControlFlow::Continue(())
        })
        .limit(1)
        .for_each()
        .unwrap();
        assert_eq!(seen, ["1"], "{engine}");
    });
}

#[test]
fn limit_applies_to_the_handler_registered_just_before_it() {
    let xml = "<r><i n='1'/><i n='2'/><j/><j/><j/></r>";
    both(xml, |engine, t| {
        let (mut is, mut js) = (0, 0);
        t.on("//i", |_| is += 1)
            .limit(1)
            .on("//j", |_| js += 1)
            .for_each()
            .unwrap();
        assert_eq!((is, js), (1, 3), "{engine}");
    });
}

#[test]
fn limit_zero_never_calls_the_handler() {
    let xml = "<r><i/></r>";
    both(xml, |engine, t| {
        let mut calls = 0;
        t.on("//i", |_| calls += 1).limit(0).for_each().unwrap();
        assert_eq!(calls, 0, "{engine}");
    });
}

#[test]
fn limit_with_context_handler() {
    let xml = "<r><i n='1'/><i n='2'/></r>";
    let mut seen = Vec::new();
    Transformer::from(xml)
        .on_with_context("//i", |node, ctx| {
            seen.push((node.get_attribute("n").unwrap(), ctx.depth()))
        })
        .limit(1)
        .for_each()
        .unwrap();
    assert_eq!(seen, [("1".to_string(), 2)]);
}

#[test]
fn limit_under_fallback() {
    let xml = "<r><g><i n='1'/><i n='2'/></g><g><i n='3'/><i n='4'/></g></r>";
    let mut seen = Vec::new();
    Transformer::from(xml)
        .allow_fallback()
        .on("//i[last()]", |node| {
            seen.push(node.get_attribute("n").unwrap())
        })
        .limit(1)
        .for_each()
        .unwrap();
    assert_eq!(seen, ["2"]);
}

#[test]
fn limit_without_a_handler_is_an_error() {
    let result = Transformer::from("<r/>")
        .limit(1)
        .on("//r", |_| {})
        .for_each();
    assert!(result.is_err(), "{result:?}");
}

// ---------------------------------------------------------------------------
// Early stop of input reading
// ---------------------------------------------------------------------------

/// Well-formed up to the third `<i/>`, broken afterwards.
const BROKEN_TAIL: &str = "<r><i/><i/><i/><oops></r>";

#[test]
fn reading_stops_once_every_handler_has_stopped() {
    both(BROKEN_TAIL, |engine, t| {
        let mut calls = 0;
        let result = t.on("//i", |_| calls += 1).limit(2).for_each();
        assert!(result.is_ok(), "{engine}: {result:?}");
        assert_eq!(calls, 2, "{engine}");
    });
    both(BROKEN_TAIL, |engine, t| {
        let result = t.on_match("//i", |_| ControlFlow::Break(())).for_each();
        assert!(result.is_ok(), "{engine}: {result:?}");
    });
}

#[test]
fn reading_continues_while_any_handler_is_active() {
    both(BROKEN_TAIL, |engine, t| {
        let result = t
            .on("//i", |_| {})
            .limit(1)
            .on_match("//i", |_| ControlFlow::Continue(()))
            .for_each();
        assert!(
            result.is_err(),
            "{engine}: expected the broken tail to be read"
        );
    });
}

#[test]
fn without_a_stop_condition_the_whole_input_is_read() {
    both(BROKEN_TAIL, |engine, t| {
        let result = t.on("//i", |_| {}).for_each();
        assert!(result.is_err(), "{engine}");
    });
}

// ---------------------------------------------------------------------------
// Transform terminals reject stop conditions and read-only handlers
// ---------------------------------------------------------------------------

#[test]
fn write_to_rejects_limit_and_on_match() {
    both("<r><i/></r>", |engine, t| {
        let result = t.on("//i", |_| {}).limit(1).to_string();
        assert!(result.is_err(), "{engine}: limit: {result:?}");
    });
    both("<r><i/></r>", |engine, t| {
        let result = t.on_match("//i", |_| ControlFlow::Continue(())).to_string();
        assert!(result.is_err(), "{engine}: on_match: {result:?}");
    });
}
