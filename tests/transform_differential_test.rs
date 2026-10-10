//! Differential test: every XPath the transform analyzer classifies as
//! streamable must select exactly the elements the DOM XPath evaluator selects.
//!
//! Non-streamable expressions must be rejected (never silently approximated).

use fastxml::transform::{Transformer, is_streamable};
use fastxml::{Parser, QueryExt};

const PLAIN: &str = r#"<root><a id="a1"><item id="1"/><item id="2" k="x"/></a><b id="b1"><item id="3" k="y"><item id="4"/></item></b><items id="s1"><item id="5"/></items><items id="s2"><item id="6"/><other id="o1"/><item id="7" k="x"/></items></root>"#;

const MIXED: &str = r#"<root><x id="x1"/><item id="1"/><x id="x2"/><item id="2" k="x"/><item id="3"/><g><x id="x3"/><x id="x4"><item id="4"/></x></g></root>"#;

const CITYGML: &str = r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0" xmlns:gml="http://www.opengis.net/gml"><core:cityObjectMember><bldg:Building gml:id="b1"><bldg:measuredHeight>10</bldg:measuredHeight></bldg:Building></core:cityObjectMember><core:cityObjectMember><bldg:Building gml:id="b2"><bldg:measuredHeight>20</bldg:measuredHeight></bldg:Building></core:cityObjectMember></core:CityModel>"#;

fn label(qname: String, id: Option<String>) -> String {
    format!("{}#{}", qname, id.unwrap_or_default())
}

fn dom_select(xml: &str, xpath: &str) -> Vec<String> {
    let doc = Parser::from(xml).parse().unwrap();
    doc.query_nodes(xpath)
        .unwrap_or_else(|e| panic!("DOM evaluation of {xpath} failed: {e}"))
        .iter()
        .filter(|n| n.is_element())
        .map(|n| label(n.qname(), n.get_attribute("id")))
        .collect()
}

/// Elements visited by `for_each` (streaming only: no fallback).
fn transform_select(xml: &str, xpath: &str) -> Vec<String> {
    let mut got = Vec::new();
    Transformer::from(xml)
        .on(xpath, |n| got.push(label(n.qname(), n.get_attribute("id"))))
        .for_each()
        .unwrap_or_else(|e| panic!("transform of {xpath} failed: {e}"));
    got
}

/// Elements modified by the in-memory transform output.
fn transform_marked(xml: &str, xpath: &str) -> usize {
    let out = Transformer::from(xml)
        .on(xpath, |n| n.set_attribute("marked", "1"))
        .to_string()
        .unwrap_or_else(|e| panic!("transform of {xpath} failed: {e}"));
    let doc = Parser::from(out.as_str()).parse().unwrap();
    doc.query_nodes("//*[@marked='1']").unwrap().len()
}

/// Matches that do not nest inside another match (for_each/transform skip
/// matches inside an already-matched subtree).
fn outermost(xml: &str, xpath: &str) -> Vec<String> {
    let doc = Parser::from(xml).parse().unwrap();
    let nodes = doc.query_nodes(xpath).unwrap();
    let ids: Vec<usize> = nodes.iter().map(|n| n.id()).collect();
    nodes
        .iter()
        .filter(|n| {
            let mut parent = n.get_parent();
            while let Some(p) = parent {
                if ids.contains(&p.id()) {
                    return false;
                }
                parent = p.get_parent();
            }
            true
        })
        .map(|n| label(n.qname(), n.get_attribute("id")))
        .collect()
}

fn check(xml: &str, xpath: &str) {
    if !is_streamable(xpath) {
        // Without fallback a non-streamable expression is an error, never a
        // silent approximation.
        let r = Transformer::from(xml).on(xpath, |_| {}).for_each();
        assert!(r.is_err(), "{xpath}: not streamable but for_each succeeded");
        return;
    }
    let expected = outermost(xml, xpath);
    assert_eq!(
        transform_select(xml, xpath),
        expected,
        "{xpath}: streaming selection differs from the DOM evaluator ({:?})",
        dom_select(xml, xpath)
    );
    assert_eq!(
        transform_marked(xml, xpath),
        expected.len(),
        "{xpath}: transform output modified a different set of elements"
    );
}

const PLAIN_XPATHS: &[&str] = &[
    "//item",
    "//item[1]",
    "//item[2]",
    "//item[last()]",
    "//item[@k='x']",
    "//item[@k!='x']",
    "//item[@k]",
    "//item[@id>3]",
    "//item[@id=3]",
    "//item[@id='3']",
    "//item[position()!=1]",
    "//item[position()<=1]",
    "//item[position()>1]",
    "//a/item",
    "//b//item",
    "//items/item",
    "//items/item[2]",
    "//items[2]/item",
    "/root//item",
    "/root/a/item",
    "/root/items[2]/item",
    "/root/items[2]/item[2]",
    "/root/*[2]/item",
    "/root/descendant::item",
    "//item[text()='x']",
    "//*[1]",
    "//*[2]",
    "/root/*",
    "/root/*[3]",
    "//a/following-sibling::b",
    "//text()",
    "//item[@k='x'][1]",
    "//item[1][@k='x']",
    "//items[@id='s2']/item[1]",
    "//items[@id='s2']/*[2]",
    "//*[local-name()='item']",
    "//b/item/item",
    "/root/b//item",
    "item",
    "a/item",
    "//item[@id='1' or @id='2']",
    "//item[not(@k)]",
    "/root/self::*",
];

const MIXED_XPATHS: &[&str] = &[
    "/root/*[2]",
    "/root/item[2]",
    "/root/x[2]",
    "//*[1]",
    "//x[2]",
    "//x[1]/item",
    "//g/x[2]/item",
    "//item[position()<3]",
    "//item[position()>=2]",
    "/root/*[position()>3]",
];

const CITYGML_XPATHS: &[&str] = &[
    "//bldg:Building",
    "//bldg:Building[@gml:id='b2']",
    "//bldg:Building[@gml:id!='b2']",
    "//bldg:Building[@gml:id]",
    "//core:cityObjectMember[2]/bldg:Building",
    "/core:CityModel/core:cityObjectMember/bldg:Building",
    "/core:CityModel//bldg:measuredHeight",
    "//bldg:Building/bldg:measuredHeight",
    "//bldg:*",
    "//*[namespace-uri()='http://www.opengis.net/citygml/building/2.0']",
    "//*[namespace-uri()='http://www.opengis.net/citygml/building/2.0'][local-name()='Building']",
];

#[test]
fn streamable_xpaths_select_like_the_dom_plain() {
    for xpath in PLAIN_XPATHS {
        check(PLAIN, xpath);
    }
}

#[test]
fn streamable_xpaths_select_like_the_dom_mixed_siblings() {
    for xpath in MIXED_XPATHS {
        check(MIXED, xpath);
    }
}

#[test]
fn streamable_xpaths_select_like_the_dom_namespaced() {
    for xpath in CITYGML_XPATHS {
        check(CITYGML, xpath);
    }
}

/// The common shapes must stay streamable (so the differential test above
/// actually compares them instead of only checking that they are rejected).
#[test]
fn common_shapes_are_streamable() {
    for xpath in [
        "//a/item",
        "//b//item",
        "/root//item",
        "/root/descendant::item",
        "/root/items[2]/item",
        "//*[1]",
        "//item[1][@k='x']",
        "//item[@k!='x']",
        "//bldg:Building[@gml:id='b2']",
        "//bldg:*",
        "item",
    ] {
        assert!(is_streamable(xpath), "{xpath} should be streamable");
    }
    for xpath in [
        "//item[@id>3]",
        "//item[@id=3]",
        "//item[position()!=1]",
        "//item[@k='x'][1]",
        "//a/following-sibling::b",
        "//text()",
    ] {
        assert!(!is_streamable(xpath), "{xpath} should not be streamable");
    }
}

#[test]
fn prefixed_attribute_predicate_removes_only_selected_building() {
    let out = Transformer::from(CITYGML)
        .on("//bldg:Building[@gml:id='b2']", |n| n.remove())
        .to_string()
        .unwrap();
    assert!(out.contains(r#"gml:id="b1""#), "{out}");
    assert!(!out.contains(r#"gml:id="b2""#), "{out}");
}
