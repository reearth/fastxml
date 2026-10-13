//! Differential test: the pre-dispatch engines (`legacy_*`) and the
//! dispatch-backed ones must report the same matches, subtrees, contexts and
//! counts, and fail on the same inputs. Removed together with the legacy
//! copies before merge.

use std::collections::HashMap;

use super::super::context::TransformContext;
use super::super::editable::EditableNode;
use super::super::error::TransformResult;
use super::*;
use crate::transform::xpath_analyze::{XPathAnalysis, analyze_xpath};
use crate::xpath::parser::parse_xpath;

const DOCS: &[&str] = &[
    r#"<root><item id="1">A</item><item id="2">B<sub>x</sub></item><other/></root>"#,
    r#"<r xmlns:b="urn:b"><b:i n="1"><b:i n="2"/><c>t</c></b:i><b:i n="3"/><i n="4"/></r>"#,
    r#"<r xmlns="urn:d"><a k="1"><a k="2"><a k="3"/></a></a><!--c--><?pi d?><a k="4"><![CDATA[x<y]]></a></r>"#,
    r#"<a><b><c/><c/></b><b><c/></b><b/></a>"#,
    r#"<r><i>1 &amp; 2</i><i a="&lt;"/>tail</r>"#,
    r#"<r><i/><i/><oops></r>"#,
    r#"<r><i></r>"#,
];

const XPATHS: &[&str] = &[
    "//item",
    "//item[@id='2']",
    "/root/item[2]",
    "//b:i",
    "//b:i[@n='1']",
    "//i",
    "//*",
    "//d:a",
    "//d:a[1]",
    "/a/b/c",
    "//b[c]",
    "//c[position() <= 1]",
    "//sub",
    "//a",
    "//b",
    "//c",
    "//*[@n]",
    "//*[@k='2']",
    "/r/i",
    "//i[2]",
    "//d:a/d:a",
    "//b:i/c",
];

fn namespaces() -> HashMap<String, String> {
    HashMap::from([
        ("b".to_string(), "urn:b".to_string()),
        ("d".to_string(), "urn:d".to_string()),
    ])
}

fn streamable(xpath: &str) -> Option<StreamableXPath> {
    match analyze_xpath(&parse_xpath(xpath).ok()?) {
        XPathAnalysis::Streamable(s) => Some(s),
        XPathAnalysis::NotStreamable(_) => None,
    }
}

fn describe(node: &EditableNode) -> String {
    node.to_xml()
        .unwrap_or_else(|e| format!("<serialize error {e}>"))
}

fn describe_ctx(node: &EditableNode, ctx: &TransformContext) -> String {
    format!(
        "{} @{} #{} {}",
        describe(node),
        ctx.depth(),
        ctx.position(),
        ctx.path_id()
    )
}

/// Outcome of one run: the callbacks' descriptions, then the count or error.
fn run(f: impl FnOnce(&mut Vec<String>) -> TransformResult<usize>) -> (Vec<String>, String) {
    let mut seen = Vec::new();
    let result = f(&mut seen);
    let tail = match result {
        Ok(n) => format!("ok {n}"),
        Err(e) => format!("err {e}"),
    };
    (seen, tail)
}

#[test]
fn single_handler_engines_agree() {
    let ns = namespaces();
    let mut compared = 0;
    let mut matched = 0;
    for doc in DOCS {
        for xpath in XPATHS {
            let Some(x) = streamable(xpath) else { continue };
            let case = format!("{xpath} on {doc}");
            compared += 1;

            let old =
                run(|s| legacy_single::process_for_each(doc, &x, &ns, |n| s.push(describe(n))));
            let new = run(|s| single::process_for_each(doc, &x, &ns, |n| s.push(describe(n))));
            assert_eq!(old, new, "process_for_each: {case}");

            let old = run(|s| {
                legacy_single::process_for_each_with_context(doc, &x, &ns, |n, c| {
                    s.push(describe_ctx(n, c))
                })
            });
            let new = run(|s| {
                single::process_for_each_with_context(doc, &x, &ns, |n, c| {
                    s.push(describe_ctx(n, c))
                })
            });
            assert_eq!(old, new, "process_for_each_with_context: {case}");

            let old = run(|s| {
                legacy_reader::process_for_each_from_reader(doc.as_bytes(), &x, &ns, |n| {
                    s.push(describe(n))
                })
            });
            let new = run(|s| {
                reader::process_for_each_from_reader(doc.as_bytes(), &x, &ns, |n| {
                    s.push(describe(n))
                })
            });
            assert_eq!(old, new, "process_for_each_from_reader: {case}");
            matched += usize::from(!old.0.is_empty());
        }
    }
    assert!(
        compared >= 70 && matched >= 30,
        "{compared} cases, {matched} with matches"
    );
}

#[test]
fn multi_handler_engines_agree() {
    let ns = namespaces();
    let pairs: Vec<(StreamableXPath, StreamableXPath)> = XPATHS
        .iter()
        .zip(XPATHS.iter().rev())
        .filter_map(|(a, b)| Some((streamable(a)?, streamable(b)?)))
        .collect();
    assert!(pairs.len() >= 8, "{} pairs", pairs.len());
    for doc in DOCS {
        for (a, b) in &pairs {
            let case = format!("{a:?} + {b:?} on {doc}");

            let old = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("a {}", describe(n)));
                let mut fb =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("b {}", describe(n)));
                let mut hs: Vec<MultiHandler<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                legacy_multi::process_for_each_multi(doc, &mut hs, &ns)
            });
            let new = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("a {}", describe(n)));
                let mut fb =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("b {}", describe(n)));
                let mut hs: Vec<MultiHandler<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                multi::process_for_each_multi(doc, &mut hs, &ns)
            });
            assert_eq!(old, new, "process_for_each_multi: {case}");

            let old = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa = |n: &mut EditableNode, c: &TransformContext| {
                    s.borrow_mut().push(format!("a {}", describe_ctx(n, c)))
                };
                let mut fb = |n: &mut EditableNode, c: &TransformContext| {
                    s.borrow_mut().push(format!("b {}", describe_ctx(n, c)))
                };
                let mut hs: Vec<MultiHandlerWithContext<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                legacy_multi::process_for_each_multi_with_context(doc, &mut hs, &ns)
            });
            let new = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa = |n: &mut EditableNode, c: &TransformContext| {
                    s.borrow_mut().push(format!("a {}", describe_ctx(n, c)))
                };
                let mut fb = |n: &mut EditableNode, c: &TransformContext| {
                    s.borrow_mut().push(format!("b {}", describe_ctx(n, c)))
                };
                let mut hs: Vec<MultiHandlerWithContext<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                multi::process_for_each_multi_with_context(doc, &mut hs, &ns)
            });
            assert_eq!(old, new, "process_for_each_multi_with_context: {case}");

            let old = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("a {}", describe(n)));
                let mut fb =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("b {}", describe(n)));
                let mut hs: Vec<MultiHandler<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                legacy_reader::process_for_each_multi_from_reader(doc.as_bytes(), &mut hs, &ns)
            });
            let new = run(|s| {
                let s = std::cell::RefCell::new(s);
                let mut fa =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("a {}", describe(n)));
                let mut fb =
                    |n: &mut EditableNode| s.borrow_mut().push(format!("b {}", describe(n)));
                let mut hs: Vec<MultiHandler<'_>> = vec![(a, &mut fa), (b, &mut fb)];
                reader::process_for_each_multi_from_reader(doc.as_bytes(), &mut hs, &ns)
            });
            assert_eq!(old, new, "process_for_each_multi_from_reader: {case}");
        }
    }
}
