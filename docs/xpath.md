# XPath

fastxml evaluates full XPath 1.0 against a parsed document. Streaming
transforms accept a streamable subset; see [Stream transform](transform.md).

## Basic usage

```rust
use fastxml::{Parser, QueryExt};

let doc = Parser::from(xml).parse()?;
let result = doc.query("//item[@id='1']/text()")?;
```

## Namespaces

```rust
let xml = r#"
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                xmlns:gml="http://www.opengis.net/gml">
    <bldg:Building gml:id="bldg_001">
        <bldg:measuredHeight>25.5</bldg:measuredHeight>
    </bldg:Building>
</core:CityModel>"#;

let doc = Parser::from(xml).parse()?;
let buildings = doc.query_nodes("//bldg:Building")?;
```

Prefixes declared on the document's root element are registered
automatically; add other bindings with `.namespace(prefix, uri)` on a
[`Query`](#reusable-queries).

## Reusable queries

A string expression is parsed again on every call. To run the same
expression against many documents, compile it once with `Query`:

```rust
use fastxml::{Parser, Query};

let query = Query::compile("//item")?;

let a = Parser::from("<root><item/><item/></root>").parse()?;
let b = Parser::from("<root><item/></root>").parse()?;

assert_eq!(query.find_nodes(&a)?.len(), 2);
assert_eq!(query.find_nodes(&b)?.len(), 1);
```

Namespaces declared on each document's root are registered automatically; add
extra bindings with `.namespace(prefix, uri)`. Use `.eval(&doc)` for a typed
`XPathResult`, or `.eval_from(&doc, &node)` to start from a context node. A
compiled `Query` (and `StreamableQuery`) renders back to an equivalent XPath
string via `to_string()`.

The `QueryExt` trait adds method-call ergonomics on the document itself. Its
argument is anything that is `AsQuery`, so a string and a pre-compiled `Query`
are interchangeable:

```rust
use fastxml::{Parser, Query, QueryExt};

let doc = Parser::from("<root><item/><item/></root>").parse()?;

// String: compiled on the fly.
assert_eq!(doc.query_nodes("//item")?.len(), 2);
let n = doc.query("count(//item)")?.to_number();

// Pre-compiled query: reused without re-parsing.
let q = Query::compile("//item")?;
assert_eq!(doc.query_nodes(&q)?.len(), 2);
```

## Supported XPath 1.0 features

| Feature | Examples |
|---------|----------|
| Paths | `/root/child`, `//element`, `//*` |
| Predicates | `[@id='1']`, `[position()=1]`, `[name()='foo']` |
| Axes | `ancestor::`, `following-sibling::`, `namespace::` |
| Operators | `and`, `or`, `not()`, `=`, `!=`, `<`, `>`, `+`, `-`, `*`, `div`, `mod` |
| Functions | `count()`, `contains()`, `string()`, `number()`, `sum()`, etc. |
| Namespaces | `//ns:element`, `namespace::*` |
| Variables | `$var` |
| Union | `//a | //b` |
| Filter expressions | `(//a)[1]`, `(//a)/text()`, `$nodes[@id]` |
| Node types | `text()`, `node()`, `comment()`, `processing-instruction('pi')` |

XPath behaviour is checked by unit tests and, with the `compare-libxml`
feature, against libxml2's results. No external XPath conformance suite is
run.
