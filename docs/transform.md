# Stream transform

`Transformer` rewrites or inspects selected elements of an XML document in a
single streaming pass. Elements are selected with a streamable subset of XPath;
see [XPath streamability check](#xpath-streamability-check) for what qualifies.

Transform XML with XPath-based element selection:

```rust
use fastxml::transform::Transformer;

let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

// Modify elements (supports multiple handlers), render the result as a String
let result = Transformer::from(xml)
    .on("//item[@id='2']", |node| node.set_attribute("modified", "true"))
    .to_string()?;

// Iterate for side effects (no output transformation)
let mut ids = Vec::new();
Transformer::from(xml)
    .on("//item", |node| {
        ids.push(node.get_attribute("id").unwrap_or_default());
    })
    .for_each()?;
```

Terminals: `to_string()`, `into_bytes()`, `write_to(&mut writer)`, and `for_each()`.

`on` / `on_with_context` / `collect` accept either a string (analyzed when the
transform runs) or a pre-compiled `StreamableQuery`. Compiling validates
streamability up front, so a non-streamable pattern is rejected immediately
rather than failing mid-run:

```rust
use fastxml::transform::{StreamableQuery, Transformer};

let q = StreamableQuery::compile("//item")?;          // Ok: streamable
assert!(StreamableQuery::compile("//item[last()]").is_err()); // rejected up front

let result = Transformer::from(xml)
    .on(&q, |node| node.set_attribute("seen", "1"))
    .to_string()?;
```

(`Query` is the analogue for *evaluation*; `StreamableQuery` is for *transforms*.)
A `StreamableQuery` is a subset of a full `Query`, so it converts freely to one
(`Query::from(&sq)`, or `doc.query(&sq)`); the reverse is fallible
(`StreamableQuery::try_from(&query)`, which rejects non-streamable expressions).

## Reader-based Transform (Large Files)

For large XML files, use `Transformer::from_reader` to avoid loading the entire file into memory. It reads from any `BufRead` source and writes results incrementally:

```rust
use fastxml::transform::Transformer;
use std::io::{BufReader, BufWriter};
use std::fs::File;

let reader = BufReader::new(File::open("large_file.xml")?);
let mut output = BufWriter::new(File::create("output.xml")?);

// Transform and write to output (returns the number of matched elements)
let count = Transformer::from_reader(reader)
    .on("//item[@id='2']", |node| node.set_attribute("modified", "true"))
    .write_to(&mut output)?;

println!("Transformed {} elements", count);

// Or iterate for side effects only (no output)
let reader = BufReader::new(File::open("large_file.xml")?);
let mut ids = Vec::new();
Transformer::from_reader(reader)
    .on("//item", |node| {
        ids.push(node.get_attribute("id").unwrap_or_default());
    })
    .for_each()?;
```

## Read-only Matches and Early Stopping

`on` buffers each matched element's whole subtree so the callback can edit it. When you only need the element's own name and attributes, use `on_match`: it is called at the start tag, buffers nothing, and reports nested matches too. Return `ControlFlow::Break(())` to stop that handler, or cap any handler with `.limit(n)`. Once every handler has stopped, the rest of the input is not read.

```rust
use std::ops::ControlFlow;
use fastxml::transform::Transformer;

let reader = BufReader::new(File::open("large_file.xml")?);
let mut buildings = Vec::new();
Transformer::from_reader(reader)
    .namespace("bldg", "http://www.opengis.net/citygml/building/2.0")
    .on_match("//bldg:Building", |m| {
        buildings.push(m.get_attribute("gml:id").unwrap_or_default().to_string());
        ControlFlow::Continue(())
    })
    .limit(10) // the first 10 buildings only
    .for_each()?;
```

`on_match` and `limit` work with `for_each` only; the output terminals (`write_to`, `to_string`, `into_bytes`) must copy the whole document and return an error.

## Advanced transforms

These richer operations are available for in-memory input (`Transformer::from`): single-pass data extraction, multi-XPath collection, parent-context access, root-namespace auto-detection, and fallback for non-streamable XPath. (On `Transformer::from_reader` they return an error, since they need random access.)

```rust
use fastxml::transform::Transformer;

let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

// Extract data (single XPath)
let ids: Vec<String> = Transformer::from(xml)
    .collect("//item", |node| node.get_attribute("id").unwrap_or_default())?;

// Extract from multiple XPaths in a single pass
let (ids, contents): (Vec<String>, Vec<String>) = Transformer::from(xml)
    .collect_multi((
        ("//item", |node| node.get_attribute("id").unwrap_or_default()),
        ("//item", |node| node.get_content().unwrap_or_default()),
    ))?;
```

## Auto-detect Namespaces

Extract namespace declarations from the root element without DOM parsing:

```rust
let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point/></root>"#;

Transformer::from(xml)
    .with_root_namespaces()?  // Auto-registers namespaces from root element
    .on("//gml:point", |node| node.set_attribute("found", "true"))
    .to_string()?;
```

## Namespace URI Matching

Match elements by namespace URI instead of prefix (useful when different prefixes map to the same URI):

```rust
// Matches both gml:feature and g:feature if they have the same namespace URI
Transformer::from(xml)
    .namespace("gml", "http://www.opengis.net/gml")
    .on("//*[namespace-uri()='http://www.opengis.net/gml'][local-name()='feature']", |node| {
        // Matches any prefix that maps to this URI
    })
    .to_string()?;
```

Prefixed name tests are matched by namespace URI too: element prefixes are resolved from the document's own `xmlns` declarations, so with `.namespace("gml", "http://www.opengis.net/gml")`, `//gml:feature` also matches `<g:feature>` when `g` is bound to that URI.

## Parent Context Access

Access ancestor elements' information during streaming transformation:

```rust
Transformer::from(xml)
    .on_with_context("//item", |node, ctx| {
        // Get parent element info
        if let Some(parent) = ctx.parent() {
            node.set_attribute("parent_name", &parent.name);
        }

        // Get path-based ID (e.g., "root/items/item[2]")
        let path = ctx.path_id();
        node.set_attribute("path", &format!("{}/item[{}]", path, ctx.position()));
    })
    .to_string()?;
```

## XPath Streamability Check

Check if an XPath can be processed in a single streaming pass:

```rust
use fastxml::transform::{is_streamable, analyze_xpath_str, XPathAnalysis};

// Quick check
if is_streamable("//item[@id='1']") {
    println!("Single-pass streaming OK");
}

// Detailed analysis
match analyze_xpath_str("//item[last()]")? {
    XPathAnalysis::Streamable(_) => println!("Streamable"),
    XPathAnalysis::NotStreamable(reason) => {
        println!("Not streamable: {}", reason);
        // Output: "Not streamable: uses last() function which requires knowing total count"
    }
}
```

## Fallback Control

By default, non-streamable XPath expressions return an error. Enable fallback for two-pass processing:

```rust
// Default: error on non-streamable XPath
let result = Transformer::from(xml)
    .on("//item[last()]", |_| {})
    .to_string();
// => Err(NotStreamable { ... })

// Enable fallback (loads entire document into memory)
let result = Transformer::from(xml)
    .allow_fallback()
    .on("//item[last()]", |_| {})
    .to_string()?;
```
