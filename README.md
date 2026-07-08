# fastxml

[![CI](https://github.com/reearth/fastxml/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/fastxml/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fastxml.svg)](https://crates.io/crates/fastxml)
[![docs.rs](https://docs.rs/fastxml/badge.svg)](https://docs.rs/fastxml)
[![License](https://img.shields.io/crates/l/fastxml.svg)](LICENSE)

A fast, memory-efficient XML library for Rust with XPath and schema validation support. Designed for processing large XML documents like CityGML files used in [PLATEAU](https://www.mlit.go.jp/plateau/).

## Features

- 🦀 **Pure Rust** — No C dependencies, no unsafe code
- 🔄 **libxml Compatible** — Consistent parsing/XPath results
- 💾 **Memory Efficient** — Stream gigabyte-scale XML with ~1.5 MB constant memory; streaming validation independent of document size
- 🔍 **Full XPath 1.0** — Complete XPath 1.0 support with namespace handling
- 📋 **XSD Support** — Schema parsing with import resolution, built-in GML types
- ⚡ **Async Support** — Async schema fetching and resolution with tokio

> ⚠️ **Early Development (v0.x)**: API may change. Limited production experience. Not recommended for business-critical systems. Use at your own risk.

## Performance

Benchmark results on current main (post-v0.9.0), PLATEAU building CityGML
(Setagaya, 61 MB, 2.1M nodes; 51 XSD schemas / 1,994 types resolved) —
[benchmark code](examples/bench.rs):

**Parse only:**

| Mode | Time | Throughput | Memory |
|------|------|------------|--------|
| libxml DOM | 0.22s | 273 MB/s | 405 MB |
| fastxml DOM | 0.35s | 174 MB/s | **367 MB** |
| fastxml Streaming | 0.27s | 223 MB/s | **~1.5 MB** |

**Parse + Schema Validation:**

| Mode | Time | Throughput | Memory |
|------|------|------------|--------|
| libxml DOM + validate | 0.22s | 272 MB/s | 405 MB |
| fastxml DOM + validate | 1.7s | 36 MB/s | 370 MB |
| fastxml Streaming + validate | 1.4s | 44 MB/s | **~60 MB** |

The libxml figures and the building-file numbers above are from that earlier
run and are kept unchanged. Streaming-validation throughput has since been
improved ~2.4x; the following is a same-machine before/after on a
schema-rich CityGML DEM tile (PLATEAU, 53 XSD schemas / 2,635 types
resolved), measured with `examples/bench.rs`:

| Streaming + validate (DEM) | Before | After | Parse-only |
|----------------------------|-------:|------:|-----------:|
| 15 MB tile  | 43.2 MB/s | **105.4 MB/s** | 183 MB/s (unchanged) |
| 220 MB tile | 43.5 MB/s | **105.6 MB/s** | 183 MB/s (unchanged) |

Two steps drove the gains. First, per-element string hashing/allocation was
replaced with interned integer symbols: names are interned once into
`SymbolId`s, per-element type resolution (element lookup, flattened children,
inline child types, text-validation plan) is memoized by symbol, and the
content-model automaton matches children by symbol binary search instead of
string-set probes — lifting the DEM tile from ~43 to ~85 MB/s. Second, a
value-check fast path (this release) handles the common case that dominates
CityGML: an *unconstrained* numeric scalar or list (a `gml:posList` /
`gml:coordinates` text node is one string of tens of thousands of
whitespace-separated doubles). Instead of collapse-normalizing the whole
string, splitting it, and running a compiled regex per item, a lean scanner
tokenizes on whitespace and lexes each token in one allocation-free pass;
any rejection defers to the canonical path, so error messages are unchanged.
This lifted the tile from ~85 to ~105 MB/s. Memory is unchanged (≈18 MB
delta on the 220 MB tile, before and after) — the caches and the symbol
table are bounded by schema size, not document size. The parse path is
untouched (parse-only throughput is flat), and the W3C XML/XSD conformance
outcomes are byte-for-byte identical before and after.

Where the remaining time goes: of the ~9.5 ms/MB total on the 15 MB DEM
tile, parsing is ~5.5 ms/MB (a hard floor) and validation overhead is now
~4.0 ms/MB (down from 6.2). Value checking — previously the dominant ~55%
of overhead — is now ~30% (~1.2 ms/MB); it is roughly tied with residual
per-element resolution/bookkeeping (~30%), followed by automaton stepping
(~17%), text buffering (~12%), and attribute validation (~11%). (Value
checking is the stage this release changed; the split is derived from the
measured throughput delta on top of the earlier stage-by-stage
apportionment.) The parser (~183 MB/s parse-only) is the eventual hard
ceiling on the way to libxml's ~270 MB/s; the remaining gap is now split
fairly evenly between residual value checking and per-element bookkeeping.

- **fastxml DOM uses less memory than libxml** on this file (367 vs
  405 MB): nodes are 128 bytes plus a compact interned attribute list. On
  text-heavy files the gap is much larger (907 MB PLATEAU DEM, measured at
  v0.8.0: fastxml 805 MB vs libxml 4.19 GB).
- **Streaming parse** uses ~1.5 MB regardless of file size; streaming
  validation adds the compiled schema and collected errors (~60 MB here,
  dominated by the 1,994 compiled types).
- **libxml cannot fetch this schema set itself**: the CityGML 2.0 imports
  include an xAL schema URL that now answers with a redirect libxml's
  fetcher does not follow. fastxml resolves the set and exports it with a
  generated XML catalog (`fastxml::schema::export`), which the benchmark
  hands to libxml via `XML_CATALOG_FILES` — so both engines validate from
  identical, fully offline schema sets. The same catalog works with
  `xmllint --nonet`.
- On this file fastxml's streaming validator reports **0 errors**; libxml
  reports one false positive (it fails to apply the
  `app:appearanceMember → gml:featureMember` substitution group at the
  document root).

Errors collected during validation share interned strings (messages,
element paths, expected/found values), so error-dense documents stay
memory-bounded.

## Installation

```toml
[dependencies]
fastxml = "0.10"
```

### Cargo Features

| Feature | Description |
|---------|-------------|
| `ureq` | Sync HTTP client for schema fetching (recommended) |
| `tokio` | Async HTTP client for schema fetching (reqwest + tokio) |
| `async-trait` | Async trait support for custom implementations |
| `compare-libxml` | Enable libxml2 comparison tests |

```toml
# Recommended: sync schema fetching
fastxml = { version = "0.10", features = ["ureq"] }

# Async schema fetching
fastxml = { version = "0.10", features = ["tokio"] }
```

### Schema Fetchers

| Fetcher | Description |
|---------|-------------|
| `FileFetcher` | Local filesystem |
| `UreqFetcher` | Sync HTTP (requires `ureq`) |
| `ReqwestFetcher` | Async HTTP (requires `tokio`) |
| `DefaultFetcher` | File + sync HTTP combined with built-in caching (requires `ureq` for HTTP) |
| `AsyncDefaultFetcher` | File + async HTTP combined with built-in caching (requires `tokio`) |
| `CachingFetcher` | Wraps any sync fetcher with in-memory caching |
| `AsyncCachingFetcher` | Wraps any async fetcher with in-memory caching (requires `tokio`) |
| `FileCachingFetcher` | Wraps any sync fetcher with file-based caching (temp directory) |
| `AsyncFileCachingFetcher` | Wraps any async fetcher with file-based caching (requires `tokio`) |

**Traits:**

| Trait | Description |
|-------|-------------|
| `SchemaFetcher` | Sync fetcher trait |
| `AsyncSchemaFetcher` | Async fetcher trait (requires `tokio`) |

```rust
use fastxml::schema::{DefaultFetcher, SchemaFetcher};

let fetcher = DefaultFetcher::with_base_dir("/path/to/schemas");
let result = fetcher.fetch("schema.xsd")?;
```

## Quick Start

### DOM Parsing

```rust
use fastxml::{Parser, QueryExt};

let xml = r#"<root><item id="1">Hello</item><item id="2">World</item></root>"#;

let doc = Parser::from(xml).parse()?;
for node in doc.query_nodes("//item")? {
    println!("{}: {}", node.get_attribute("id").unwrap(), node.get_content().unwrap());
}
```

`Parser::from` accepts `&str` or `&[u8]`; use `Parser::from_reader(reader)` to parse from any `BufRead`, and `.options(ParserOptions { .. })` to configure parsing.

### Reusable XPath Queries

`evaluate(&doc, "…")` re-parses the expression on every call. To run the same
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

### Serializing to XML

`Printer` turns a parsed document or node back into XML:

```rust
use fastxml::{Parser, Printer};

let doc = Parser::from("<root><child>hi</child></root>").parse()?;

let xml = Printer::from(&doc).to_string()?;            // whole document, with <?xml ?>
let pretty = Printer::from(&doc).pretty().to_string()?; // indented

// Stream straight to any writer, no intermediate String:
Printer::from(&doc).write_to(&mut std::io::stdout())?;
```

`Printer::from` accepts `&XmlDocument`, `&XmlNode`, or `&XmlRoNode` (a document
emits an XML declaration by default, a single node does not). Builders:
`.pretty()` / `.indent(s)` / `.declaration(bool)` / `.encoding(s)`. Terminals:
`.to_string()` / `.into_bytes()` / `.write_to(w)`.

### Streaming Parser

For a quick, buffered list of events:

```rust
use fastxml::Parser;

for event in Parser::from(xml).events()? {
    // inspect each XmlEvent
}
```

To process large files with **constant memory**, use `for_each_event` — the callback is invoked as each event is read, nothing is buffered, and it may capture and mutate local state:

```rust
use fastxml::Parser;
use fastxml::event::XmlEvent;
use std::io::BufReader;
use std::fs::File;

let file = File::open("large_file.xml")?;

let mut elements = 0;
Parser::from_reader(BufReader::new(file)).for_each_event(|event| {
    if let XmlEvent::StartElement { .. } = event {
        elements += 1;
    }
    Ok(())
})?;
println!("{elements} elements");
```

### Stream Transform

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

#### Reader-based Transform (Large Files)

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

#### Advanced transforms

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

#### Auto-detect Namespaces

Extract namespace declarations from the root element without DOM parsing:

```rust
let xml = r#"<root xmlns:gml="http://www.opengis.net/gml"><gml:point/></root>"#;

Transformer::from(xml)
    .with_root_namespaces()?  // Auto-registers namespaces from root element
    .on("//gml:point", |node| node.set_attribute("found", "true"))
    .to_string()?;
```

#### Namespace URI Matching

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

#### Parent Context Access

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

#### XPath Streamability Check

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

#### Fallback Control

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

## Async Schema Resolution

Parse XSD schemas with async import/include resolution (requires `tokio` feature):

```rust
use fastxml::schema::{AsyncDefaultFetcher, Schema};

#[tokio::main]
async fn main() -> fastxml::error::Result<()> {
    let xsd_content = std::fs::read("schema.xsd")?;

    // Create async fetcher
    let fetcher = AsyncDefaultFetcher::new()?;

    // Build the schema, resolving imports asynchronously
    let schema = Schema::builder()
        .add("http://example.com/schema.xsd", xsd_content)
        .resolve_with_async(&fetcher)
        .await?;

    println!("Parsed {} types", schema.types.len());
    Ok(())
}
```

`Schema::builder()` takes one or more `.add(uri, bytes)` sources; finish with `.resolve()` (no network), `.resolve_with(&fetcher)`, or `.resolve_with_async(&fetcher)`.

The async resolver:
- Fetches imported schemas asynchronously via HTTP
- Resolves nested imports (A → B → C)
- Detects circular dependencies

See [examples/async_schema_resolution.rs](examples/async_schema_resolution.rs) for more examples.

## Schema Validation

All validation goes through one `Validator` front door: the input type selects the engine (`&XmlDocument` → DOM, `&str`/`&[u8]`/reader → streaming), `.schema(..)` supplies an explicit schema (or it is resolved from `xsi:schemaLocation`), and `run()` returns a `Report`.

A `Schema` is built with `Schema::from_xsd(bytes)`, `Schema::builtin()`, or `Schema::builder().add(uri, bytes).resolve()?`.

### DOM Validation

```rust
use fastxml::Parser;
use fastxml::schema::{Schema, Validator};

let doc = Parser::from(std::fs::read("document.xml")?.as_slice()).parse()?;
let schema = Schema::from_xsd(std::fs::read("schema.xsd")?)?;

let report = Validator::from(&doc).schema(schema).run()?;

if report.is_valid() {
    println!("Valid!");
}
```

### Streaming Validation

Validate during parsing with minimal memory:

```rust
use fastxml::schema::{Schema, Validator};
use std::sync::Arc;

let schema = Arc::new(Schema::from_xsd(std::fs::read("schema.xsd")?)?);
let reader = std::io::BufReader::new(file);

let report = Validator::from_reader(reader)
    .schema(Arc::clone(&schema))   // share one schema across many validations
    .max_errors(100)
    .run()?;
```

### Aggregated Errors

On error-dense documents, `.aggregate_errors()` collapses identical errors
into one entry whose `count` records the occurrences (memory stays bounded:
a million identical violations become one entry). Every occurrence's
position is preserved — the first in `location`, the rest as compact
`(line, column)` pairs in `more_positions` — and `Display` appends the
count and line range:

```rust
let report = Validator::from_reader(reader)
    .schema(schema)
    .aggregate_errors()
    .run()?;

for error in report.errors() {
    println!("{error}"); // e.g. "[error] /root/item: ... (×4831, lines 2522-1111318)"
}
```

### Auto-detect Schema

Omit `.schema(..)` and the schema is resolved from the document's `xsi:schemaLocation`, using the default fetcher (requires the `ureq` feature):

```rust
use fastxml::{Parser, schema::Validator};

let doc = Parser::from(xml_bytes).parse()?;
let report = Validator::from(&doc).run()?;
```

For streaming, the schema is fetched lazily on the first element:

```rust
use fastxml::schema::Validator;

let report = Validator::from_reader(reader).run()?;
```

To supply a custom fetcher, use `.run_with(fetcher)` instead of `.run()`.

### Async Validation

Validate with async schema fetching (requires `tokio` feature) via `run_async()` (default fetcher) or `run_async_with(&fetcher)`:

```rust
use fastxml::{Parser, schema::Validator};

#[tokio::main]
async fn main() -> fastxml::error::Result<()> {
    let doc = Parser::from(xml_bytes).parse()?;
    let report = Validator::from(&doc).run_async().await?;
    Ok(())
}
```

### Validation Errors

```rust
use fastxml::ErrorLevel;

// `report` is the value returned by `Validator::…::run()`
for error in report.errors() {
    match error.level {
        ErrorLevel::Warning => print!("[WARN] "),
        ErrorLevel::Error => print!("[ERROR] "),
        ErrorLevel::Fatal => print!("[FATAL] "),
    }
    if let Some(line) = error.line {
        print!("line {}: ", line);
    }
    println!("{}", error.message);
}
```

## XPath

### Basic Usage

```rust
use fastxml::{Parser, QueryExt};

let doc = Parser::from(xml).parse()?;
let result = doc.query("//item[@id='1']/text()")?;
```

### With Namespaces

```rust
let xml = r#"
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
    <bldg:Building gml:id="bldg_001">
        <bldg:measuredHeight>25.5</bldg:measuredHeight>
    </bldg:Building>
</core:CityModel>"#;

let doc = Parser::from(xml).parse()?;
let buildings = doc.query_nodes("//bldg:Building")?;
```

## libxml Compatibility

For migrating from libxml, the `fastxml::compat` module provides free functions
that mirror libxml's shape (`evaluate`, `create_context`, `get_root_node`,
`node_to_xml_string`, `find_nodes_by_xpath`, …). They are thin wrappers over the
modern front doors — prefer `Parser` / `Query` / `QueryExt` / `Printer` for new
code.

```rust
use fastxml::Parser;
use fastxml::compat::{evaluate, get_root_node};

let doc = Parser::from(xml).parse()?;
let root = get_root_node(&doc)?;          // modern: doc.get_root_element()
let items = evaluate(&doc, "//item")?;    // modern: doc.query("//item")
```

See `examples/` (`query`, `printer`, `compat`, `dom_parsing`, …) for runnable
demonstrations of both the modern and compatibility APIs.

## Supported Specifications

### XPath 1.0

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

### XSD Schema

| Feature | Support |
|---------|---------|
| Element/attribute definitions | ✅ |
| Complex types (sequence/choice/all) | ✅ |
| Simple types (restriction/list/union) | ✅ |
| Type inheritance | ✅ |
| Facets | ✅ |
| Wildcards (xs:any / xs:anyAttribute) | ✅ |
| Attribute/model groups | ✅ |
| import/include | ✅ |
| redefine | ✅ |
| Built-in XSD and GML types | ✅ |
| Identity constraints (unique/key/keyref) | ✅ |
| Substitution groups | ✅ |
| Content-model automaton (choice totals, sequence-as-unit occurrence, UPA detection) | ✅ |
| XSD 1.1 datatypes (dateTimeStamp, dayTimeDuration, yearMonthDuration, explicitTimezone) | ✅ |
| Other XSD 1.1 features (assertions, conditional type assignment, openContent, override) | ❌ |

### Not Supported

- XQuery, XSLT, XInclude
- DTD validation
- XML Signature/Encryption
- Catalog support
- Full entity expansion

## Conformance

Conformance results on current `main`, measured by a harness that records one
honest outcome per test: `pass`, `fail`, `unsupported` (a feature fastxml
deliberately does not implement — XML 1.1, XML 1.0 5th-edition-only rules,
non-UTF-8 encodings), `blocked` (the harness could not decide, e.g. an
unresolvable schema import), or `panic`. The **pass rate is
`pass / (pass + fail + panic)`** — decided tests only — and **coverage** is the
share of all tests that were decided. Both engines (DOM and streaming) are run;
results are diffed against committed [baselines](conformance/baselines/) so any
change in library behaviour must land with an updated baseline. See
[conformance/](conformance/) for details.

### W3C XML — 2,585 tests, DOM engine

| Category | Pass rate | pass | fail | unsupported |
|----------|-----------|------|------|-------------|
| valid documents          | 93.2% | 381  | 28  | 403 |
| invalid documents (DTD)  | 96.7% | 206  | 7   | 29  |
| not-well-formed          | 90.8% | 1104 | 112 | 282 |
| error (optional)         | —     | 0    | 0   | 33  |
| **overall**              | **92.0%** | **1691** | **147** | **747** |

The streaming engine is within a few tests of DOM (overall 91.9%; not-well-formed
1102 pass / 114 fail). `unsupported` is dominated by XML 1.1 / 5th-edition-only
tests and non-UTF-8 encodings, which fastxml does not target (it targets XML 1.0
4th edition, UTF-8). fastxml is always namespace-aware, so the four
`NAMESPACE="no"` tests that assume a bare colon is an ordinary name character are
scored `unsupported` rather than run. Well-formedness enforcement covers the
`Char` and `Name` productions, document structure (single root, prolog/epilog
content, `]]>` in character data, unclosed elements), the full `DOCTYPE` internal
subset (PI targets and `ELEMENT` / `ATTLIST` / `ENTITY` / `NOTATION` declaration
grammar, conditional sections, `PubidLiteral` characters), character-reference
and entity semantics (malformed/illegal references, reference cycles), the XML
declaration, and the Namespaces in XML 1.0 constraints: `QName` syntax, prefix
binding, the reserved `xml` / `xmlns` namespace rules, expanded-name attribute
uniqueness, and NCName restrictions on PI targets and entity/notation names. The
remaining not-well-formed failures need character-reference / entity-reference /
attribute-value normalization before comparing namespace URIs (which the raw
per-event checker cannot do), plus constructs that require full entity expansion
and re-parsing (see the roadmap).

### W3C XSD — 39,613 tests, DOM engine

| Category | Pass rate | pass | fail | blocked |
|----------|-----------|------|------|---------|
| valid schemas accepted      | 100.0% | 11,139 | 0   | 0   |
| invalid schemas rejected    | 79.7%  | 2,671  | 681 | 0   |
| valid instances             | 99.4%  | 13,677 | 80  | 306 |
| invalid instances rejected  | 97.0%  | 10,587 | 332 | 112 |
| **overall**                 | **97.2%** | **38,074** | **1,093** | **418** |

Schema compilation stays asymmetric by design: every valid schema compiles
(zero false rejections), while 79.7% of invalid schemas are rejected. The
rejection rules cover reference integrity (dangling QName references into
fully-present namespaces), circular definitions, cos-all-limited,
identity-constraint XPath grammar, attribute/placement/lexical
schema-for-schemas rules, derivation `final` controls, compile-time facet
validity, and a particle-restriction (rcase-*) engine — each rule only fires
when its verdict is certain, so partially-resolved real-world schema sets
(CityGML/PLATEAU) keep compiling. `blocked` instances are those whose schema
could not be resolved/compiled, plus one wildcard instance whose validity is
nondeterministic in fastxml. The streaming engine scores 97.2% overall. XSD
1.1-only test groups are excluded (XSD 1.0 target); the 28 indeterminate
schema/instance tests are reported as `unsupported`.

> **Numbers from v0.9.x and earlier are not directly comparable** — the harness
> previously counted *any* error as a pass for negative tests and dropped
> blocked instances from the denominators, so both pass rates and totals were
> measured against the wrong basis.

```bash
# Run conformance tests (requires test data download)
cargo run -p fastxml-conformance --bin download
cargo test -p fastxml-conformance

# Regenerate the committed baselines after an intentional behaviour change
FASTXML_UPDATE_BASELINE=1 cargo test -p fastxml-conformance
```

## Roadmap

Known issues and planned improvements, roughly in priority order:

**Schema (XSD) coverage**

- Invalid-schema rejection (79.6%, up from 52.3%): implemented reference
  integrity, circular definitions, cos-all-limited, identity-constraint XPath
  grammar, attribute/placement/lexical rules, derivation controls, facet
  validity, and a certainty-gated rcase-* particle-restriction engine.
  Remaining gaps: attribute-use restriction legality, redefine legality,
  facet checking across user-defined base chains, MapAndSum and
  wildcard-namespace-set comparisons in the rcase engine, and rules that
  need resolved imports.
- XSD 1.1: assertions (`xs:assert`), conditional type assignment
  (`xs:alternative`), `openContent`, `xs:override`. Datatypes are done.

**XML parsing**

- Stricter not-well-formed detection (90.8% of W3C not-wf tests now pass under
  the honest denominator, up from 26%). Enforced: the `Char` and `Name`
  productions (XML 1.0 4th-edition character classes); document structure
  (exactly one root, no character data or second root outside it, no literal
  `]]>` in character data, no `<` in attribute values, no unclosed elements);
  the `DOCTYPE` internal subset (PI targets, `<!ELEMENT>` / `<!ATTLIST>` /
  `<!ENTITY>` / `<!NOTATION>` declaration grammar, rejection of conditional
  sections, `PubidLiteral` character class), with conservative acceptance when a
  parameter-entity reference could make declarations invisible; character
  references (`&#…` syntax and legality) and internal general-entity semantics
  (reference cycles, references to undeclared entities); the XML declaration
  (version / encoding / standalone, ordering, position); and the full
  Namespaces in XML 1.0 constraints (`QName` syntax, prefix binding, reserved
  `xml`/`xmlns` namespace rules, expanded-name attribute uniqueness, NCName PI
  targets and entity/notation names). Because fastxml is namespace-aware, the
  OASIS XML 1.0 `NAMESPACE="no"` tests (which require `:` to be accepted as an
  ordinary name character) are scored `unsupported`. Remaining gap: not-wf
  cases that need character-reference / entity-reference / attribute-value
  normalization before comparing namespace URIs, external DTD subsets, and
  errors that surface only after full entity expansion and re-parsing.

**XPath**

- XPath conformance is currently covered by unit tests only; no standard
  external suite (e.g. OASIS XPath 1.0) is run.

**Performance / memory**

- Validation throughput: streaming validation runs at ~105 MB/s (up from
  ~44 MB/s) vs libxml's ~270 MB/s on schema-rich CityGML. Earlier gains
  removed per-element/per-value allocations (borrowing compiled types via the
  schema `Arc` instead of cloning them, `Cow` whitespace normalization that
  borrows already-collapsed values, memoized attribute collection, and
  skipping identity bookkeeping when nothing is tracked). The **symbol-ID
  validator layer is done**: names are interned once into integer `SymbolId`s,
  per-element resolution (global element lookup, flattened children, inline
  child-type resolution, and the text-validation plan) is memoized by symbol
  instead of re-hashing/re-allocating strings on every start tag, and the
  content-model automaton matches by symbol binary search. The **numeric
  value-check fast path is also done**: unconstrained numeric scalars and
  lists (double/float/decimal/integer with `whiteSpace=collapse` and no other
  facets — the `gml:posList` / `gml:coordinates` case) skip the facet
  machinery and per-item regex for a single allocation-free byte scan,
  deferring to the canonical path on any rejection so errors are unchanged.
  This roughly halved the value-checking cost and lifted the DEM tile from
  ~85 to ~105 MB/s. What remains of the gap is now split fairly evenly
  (percentages of the reduced ~4.0 ms/MB validation overhead, derived from
  the throughput delta): **residual per-element resolution/bookkeeping**
  (~30%) and **remaining value checking** (~30%, now for bounded/sign-
  constrained integer subtypes, unions, patterns/enumerations and non-numeric
  types, which still take the canonical path), then automaton stepping (~17%),
  text buffering (~12%), and attribute validation (~11%). Next candidates:
  trimming per-element bookkeeping, and an interned event layer (the parser
  handing symbols to the validator). The parser (~183 MB/s parse-only) is the
  eventual hard ceiling: reaching libxml parity ultimately also needs faster
  tokenization on top of the above.


## Development

```bash
cargo test                              # Run tests
cargo test --features tokio             # With async tests
cargo test --features compare-libxml    # With libxml comparison
cargo bench                             # Benchmarks

# Validate XML files against XSD schema
cargo run --release --features ureq --bin fastxml-validate -- ./file.xml

# Benchmarks with an external xml file
cargo run --release --example bench -- ./file.xml
cargo run --release --features ureq --example bench -- ./file.xml --validate
```

## License

MIT OR Apache-2.0
