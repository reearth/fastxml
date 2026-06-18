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
fastxml = "0.9"
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
fastxml = { version = "0.9", features = ["ureq"] }

# Async schema fetching
fastxml = { version = "0.9", features = ["tokio"] }
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

Conformance test results on current main (post-v0.9.0). See
[conformance/](conformance/) for details.

| Test Suite | Category | Pass Rate |
|------------|----------|-----------|
| W3C XML | valid documents | 98.5% |
| W3C XML | invalid documents | 97.4% |
| W3C XSD | schema compilation | 89.0% |
| W3C XSD | instance validation (DOM) | 98.0% |
| W3C XSD | instance validation (streaming) | 97.5% |

Schema compilation breaks down asymmetrically: every valid schema in the
suite compiles (100%, zero false rejections), while only 52.3% of invalid
schemas are rejected — fastxml is permissive toward malformed schemas
rather than strict.

XSD tests are evaluated against XSD 1.0 expectations; XSD 1.1-only test
groups are excluded.

Note: numbers from v0.8.2 and earlier are not comparable — the test
catalog parser used to misread the expected outcome of each test
(`<expected validity="...">` was ignored), so both pass rates and
failure counts were measured against the wrong expectations.

```bash
# Run conformance tests (requires test data download)
cargo run -p fastxml-conformance --bin download
cargo test -p fastxml-conformance
```

## Roadmap

Known issues and planned improvements, roughly in priority order:

**Validation correctness**

- **DOM validator namespace handling**: child element declarations can be
  resolved by local name across namespaces, producing false positives on
  documents that reuse a local name in different namespaces (e.g. CityGML
  `gen:value` vs. measure `value` — ~30k spurious errors on the benchmark
  file where the streaming validator and libxml report none). The streaming
  validator is unaffected. Namespace-aware element declarations (carrying
  their target namespace through compilation) would fix this and several
  conformance tails at once.
- **Streaming identity constraints**: keyref tuples are compared lexically,
  not in the value space (the DOM validator already compares typed values).

**Schema (XSD) coverage**

- Invalid-schema rejection (52.3%): deeper particle-restriction legality
  (nested compositor mapping), dangling-reference detection, and
  schema-for-schemas edge cases.
- XSD 1.1: assertions (`xs:assert`), conditional type assignment
  (`xs:alternative`), `openContent`, `xs:override`. Datatypes are done.

**XML parsing**

- Stricter not-well-formed detection (the underlying parser accepts some
  malformed input that the W3C suite expects to be rejected).

**Performance / memory**

- Validation throughput: streaming validation runs at ~44 MB/s vs libxml's
  ~270 MB/s on schema-rich CityGML (the event pipeline is already
  zero-copy; remaining cost is per-element schema lookups and per-value
  facet checks).


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
