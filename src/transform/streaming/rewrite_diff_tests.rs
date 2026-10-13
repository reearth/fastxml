//! Temporary differential test: the `rewrite`-backed engines against copies
//! of the engines they replace (`legacy_*.rs`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::BufReader;
use std::rc::Rc;

use super::super::context::TransformContext;
use super::super::editable::EditableNode;
use super::super::error::TransformResult;
use super::super::xpath_analyze::{StreamableXPath, XPathAnalysis, analyze_xpath};
use super::{MultiTransformHandler, MultiTransformHandlerWithContext};
use super::{legacy_multi, legacy_reader, legacy_single};
use crate::xpath::parser::parse_xpath;

const DOCS: &[&str] = &[
    "",
    "<root/>",
    r#"<root><item id="1">A</item><item id="2">B</item></root>"#,
    "\u{feff}<?xml version=\"1.0\"?><root><item/></root>",
    "\u{feff}<root><item id=\"2\">x</item><b/></root>\n",
    r#"<root><item id="1"><item id="2"><b/></item></item><b>t</b></root>"#,
    r#"<root xmlns:a="urn:a"><a:item id="1">x</a:item><item/><a:b><item/></a:b></root>"#,
    r#"<root xmlns="urn:default"><item>1</item><b xmlns="">2</b><item xmlns="urn:a"/></root>"#,
    r#"<!-- c --><?pi x?><root><item><!--in--><?p d?><![CDATA[<x>]]></item><!--out--><?q?><b><![CDATA[c]]></b></root><!--tail-->"#,
    r#"<root><item a="&lt;&amp;">&lt;x&gt; &amp; &#65;</item><b>&quot;</b></root>"#,
    r#"<!DOCTYPE root [<!ENTITY e "v">]><root><item>&amp;</item></root>"#,
    "<root><item>x</item><b>",
    "<root><item>x</b></root>",
    "<root><item id=\"1>x</item></root>",
    "<root><item>x</item></root><",
    "<root><item><b></item></root>",
    "<root><item>&bogus;</item></root>",
    "<root><b>&bogus;</b><item/></root>",
    "<root><item/><item></item><b/><item>  </item></root>",
    "<root>\n  <item id=\"1\">\n    <b>x</b>\n  </item>\n  <item id='2' x = \"3\"/>\n</root>\n",
    r#"<root><b><item><b/></item></b><item><b><item/></b></item></root>"#,
    r#"<a:root xmlns:a="urn:a"><a:item><a:b/></a:item><a:item a:x="1"/></a:root>"#,
    "<root><item>日本語</item><b>é</b></root>",
    "text<root/>",
    "<root><item><?pi?></item><item><?pi  spaced content ?></item></root>",
    r#"<root xmlns:a="urn:a" xmlns:c="urn:a"><c:item/><a:item><c:b/></a:item></root>"#,
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- head -->\n<root><item id=\"1\"/><item id=\"2\"/><item id=\"3\"/></root>",
    "<root><item id=\"1\"><![CDATA[a]]>b<!--c--><?d e?>f</item></root>",
    "<root><item></root>",
    "<root></root></extra>",
];

const XPATHS: &[&str] = &[
    "//item",
    "/root/item",
    "//item[@id='2']",
    "//b",
    "/root",
    "//a:item",
    "//a:b",
    "//*",
    "//item[1]",
    "//item[2]",
    "/root/b",
    "//item/b",
    "//d:item",
    "/a:root/a:item",
    "//b/item",
    "//item[@id]",
    "/*",
];

fn namespaces() -> HashMap<String, String> {
    let mut ns = HashMap::new();
    ns.insert("a".to_string(), "urn:a".to_string());
    ns.insert("d".to_string(), "urn:default".to_string());
    ns
}

fn streamable(xpath: &str) -> Option<StreamableXPath> {
    match analyze_xpath(&parse_xpath(xpath).ok()?) {
        XPathAnalysis::Streamable(s) => Some(s),
        XPathAnalysis::NotStreamable(_) => None,
    }
}

/// Shared callback recorder: logs every call and edits the node by a
/// rotating rule, so removals, attribute and text edits all reach output.
#[derive(Clone)]
struct Recorder {
    log: Rc<RefCell<Vec<String>>>,
    calls: Rc<Cell<usize>>,
    mode: usize,
}

impl Recorder {
    fn new(mode: usize) -> Self {
        Self {
            log: Rc::default(),
            calls: Rc::default(),
            mode,
        }
    }

    fn call(&self, handler: usize, node: &mut EditableNode, ctx: Option<&TransformContext>) {
        let n = self.calls.get();
        self.calls.set(n + 1);
        self.log.borrow_mut().push(format!(
            "h{handler} {} {:?} {:?}",
            node.qname(),
            node.to_xml().map_err(|e| format!("{e:?}")),
            ctx.map(describe_context),
        ));
        match (n + self.mode) % 4 {
            0 => node.set_attribute("k", "v&\""),
            1 => node.remove(),
            2 => node.set_text_content("t&<"),
            _ => {}
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

/// The context, with attributes sorted (they are kept in a `HashMap`).
fn describe_context(ctx: &TransformContext) -> String {
    let ancestors: Vec<String> = ctx
        .ancestors()
        .iter()
        .map(|a| {
            let mut attrs: Vec<_> = a.attributes.iter().collect();
            attrs.sort();
            format!(
                "{} {} {:?} {:?} {} {}",
                a.name, a.qname, a.prefix, attrs, a.position, a.depth
            )
        })
        .collect();
    format!("{ancestors:?} {} {}", ctx.position(), ctx.depth())
}

type NodeFn = dyn FnMut(&mut EditableNode);

type Outcome = (Result<usize, String>, Vec<u8>, Vec<String>);

fn outcome(result: TransformResult<usize>, out: Vec<u8>, rec: &Recorder) -> Outcome {
    (result.map_err(|e| format!("{e:?}")), out, rec.log())
}

/// Runs one engine pair and returns whether the old engine matched anything.
fn compare(label: &str, run: impl Fn(bool, &Recorder) -> Outcome, mode: usize) -> bool {
    let (old_rec, new_rec) = (Recorder::new(mode), Recorder::new(mode));
    let old = run(true, &old_rec);
    let new = run(false, &new_rec);
    assert_eq!(
        old,
        new,
        "{label} (mode {mode}): old {:?} / new {:?}",
        String::from_utf8_lossy(&old.1),
        String::from_utf8_lossy(&new.1)
    );
    matches!(old.0, Ok(n) if n > 0)
}

fn single_cases(doc: &str, xpath: &StreamableXPath, label: &str, mode: usize) -> [bool; 3] {
    let ns = namespaces();
    let str_plain = compare(
        &format!("str {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            let f = |n: &mut EditableNode| rec.call(0, n, None);
            let r = if legacy {
                legacy_single::process_streaming(doc, xpath, &ns, f, &mut out)
            } else {
                super::process_streaming(doc, xpath, &ns, f, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    let str_ctx = compare(
        &format!("str+ctx {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            let f = |n: &mut EditableNode, c: &TransformContext| rec.call(0, n, Some(c));
            let r = if legacy {
                legacy_single::process_streaming_with_context(doc, xpath, &ns, f, &mut out)
            } else {
                super::process_streaming_with_context(doc, xpath, &ns, f, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    let reader = compare(
        &format!("reader {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            let input = BufReader::with_capacity(7, doc.as_bytes());
            let f = |n: &mut EditableNode| rec.call(0, n, None);
            let r = if legacy {
                legacy_reader::process_streaming_from_reader(input, xpath, &ns, f, &mut out)
            } else {
                super::process_streaming_from_reader(input, xpath, &ns, f, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    [str_plain, str_ctx, reader]
}

fn multi_cases(doc: &str, xpaths: &[&StreamableXPath], label: &str, mode: usize) -> [bool; 3] {
    let ns = namespaces();
    let str_plain = compare(
        &format!("multi str {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            let mut fns: Vec<Box<NodeFn>> = (0..xpaths.len())
                .map(|i| {
                    let rec = rec.clone();
                    Box::new(move |n: &mut EditableNode| rec.call(i, n, None)) as Box<_>
                })
                .collect();
            let mut handlers: Vec<MultiTransformHandler<'_>> = xpaths
                .iter()
                .zip(fns.iter_mut())
                .map(|(x, f)| (*x, &mut **f as &mut dyn FnMut(&mut EditableNode)))
                .collect();
            let r = if legacy {
                legacy_multi::process_streaming_multi(doc, &mut handlers, &ns, &mut out)
            } else {
                super::process_streaming_multi(doc, &mut handlers, &ns, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    let str_ctx = compare(
        &format!("multi str+ctx {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            type CtxFn = dyn FnMut(&mut EditableNode, &TransformContext);
            let mut fns: Vec<Box<CtxFn>> = (0..xpaths.len())
                .map(|i| {
                    let rec = rec.clone();
                    Box::new(move |n: &mut EditableNode, c: &TransformContext| {
                        rec.call(i, n, Some(c))
                    }) as Box<CtxFn>
                })
                .collect();
            let mut handlers: Vec<MultiTransformHandlerWithContext<'_>> = xpaths
                .iter()
                .zip(fns.iter_mut())
                .map(|(x, f)| {
                    (
                        *x,
                        &mut **f as &mut dyn FnMut(&mut EditableNode, &TransformContext),
                    )
                })
                .collect();
            let r = if legacy {
                legacy_multi::process_streaming_multi_with_context(
                    doc,
                    &mut handlers,
                    &ns,
                    &mut out,
                )
            } else {
                super::process_streaming_multi_with_context(doc, &mut handlers, &ns, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    let reader = compare(
        &format!("multi reader {label}"),
        |legacy, rec| {
            let mut out = Vec::new();
            let input = BufReader::with_capacity(7, doc.as_bytes());
            let mut fns: Vec<Box<NodeFn>> = (0..xpaths.len())
                .map(|i| {
                    let rec = rec.clone();
                    Box::new(move |n: &mut EditableNode| rec.call(i, n, None)) as Box<_>
                })
                .collect();
            let mut handlers: Vec<MultiTransformHandler<'_>> = xpaths
                .iter()
                .zip(fns.iter_mut())
                .map(|(x, f)| (*x, &mut **f as &mut dyn FnMut(&mut EditableNode)))
                .collect();
            let r = if legacy {
                legacy_reader::process_streaming_multi_from_reader(
                    input,
                    &mut handlers,
                    &ns,
                    &mut out,
                )
            } else {
                super::process_streaming_multi_from_reader(input, &mut handlers, &ns, &mut out)
            };
            outcome(r, out, rec)
        },
        mode,
    );
    [str_plain, str_ctx, reader]
}

#[test]
fn rewrite_matches_legacy_engines() {
    let xpaths: Vec<(&str, StreamableXPath)> = XPATHS
        .iter()
        .filter_map(|x| streamable(x).map(|s| (*x, s)))
        .collect();
    assert!(
        xpaths.len() >= 14,
        "only {} streamable xpaths",
        xpaths.len()
    );

    let (mut cases, mut with_match, mut errors) = (0usize, 0usize, 0usize);
    let mut tally = |results: [bool; 3]| {
        for matched in results {
            cases += 1;
            with_match += usize::from(matched);
        }
    };

    for doc in DOCS {
        for mode in 0..4 {
            for (label, xpath) in &xpaths {
                tally(single_cases(
                    doc,
                    xpath,
                    &format!("{label} on {doc:?}"),
                    mode,
                ));
            }
            for (i, (li, xi)) in xpaths.iter().enumerate() {
                // Pairs in both orders, plus a triple, for first-match-wins.
                let (lj, xj) = &xpaths[(i + 1) % xpaths.len()];
                let (lk, xk) = &xpaths[(i + 5) % xpaths.len()];
                tally(multi_cases(
                    doc,
                    &[xi, xj],
                    &format!("[{li}, {lj}] on {doc:?}"),
                    mode,
                ));
                tally(multi_cases(
                    doc,
                    &[xj, xi],
                    &format!("[{lj}, {li}] on {doc:?}"),
                    mode,
                ));
                tally(multi_cases(
                    doc,
                    &[xi, xj, xk],
                    &format!("[{li}, {lj}, {lk}] on {doc:?}"),
                    mode,
                ));
            }
        }
        // Errors are compared too; count the documents that produce one.
        let mut out = Vec::new();
        if legacy_single::process_streaming(doc, &xpaths[0].1, &namespaces(), |_| {}, &mut out)
            .is_err()
        {
            errors += 1;
        }
    }

    eprintln!("differential: {cases} cases, {with_match} with matches, {errors} error docs");
    assert!(cases >= 10_000, "only {cases} cases");
    assert!(with_match >= 3_000, "only {with_match} cases with matches");
    assert!(errors >= 4, "only {errors} documents fail to parse");
}
