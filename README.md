# fastxml

[![CI](https://github.com/reearth/fastxml/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/fastxml/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fastxml.svg)](https://crates.io/crates/fastxml)
[![docs.rs](https://docs.rs/fastxml/badge.svg)](https://docs.rs/fastxml)
[![License](https://img.shields.io/crates/l/fastxml.svg)](LICENSE)

A fast, memory-efficient XML library for Rust with XPath and schema validation support. Designed for processing large XML documents like CityGML files used in [PLATEAU](https://www.mlit.go.jp/plateau/).

## ⚠️ Project Status

> **This library is in early development (v0.x).** While basic functionality has been tested and benchmarked, please be aware of the following:

- **API Stability**: The API may change between minor versions without notice
- **Production Readiness**: This is a young library with limited production experience — **do not expect libxml-level maturity or reliability**
- **Use at Your Own Risk**: No warranties are provided; use this library at your own discretion
- **Not for Critical Systems**: Avoid using in business-critical or safety-critical systems where XML processing failures could cause significant harm

We welcome bug reports and contributions, but please evaluate carefully before adopting this library for production use.

## Features

- 🦀 **Pure Rust** — No C dependencies, no unsafe code
- 🔄 **libxml Compatible** — Consistent parsing/XPath results
- 💾 **Memory Efficient** — Parse and validate gigabyte-scale XML with ~1 MB memory footprint
- ✅ **Multiple Validators** — DOM, Two-Pass, and One-Pass validation strategies
- 🔍 **Full XPath 1.0** — Complete XPath 1.0 support with namespace handling
- 📋 **XSD Support** — Schema parsing with import resolution, built-in GML types
- ⚡ **Async Support** — Async schema fetching and resolution with tokio

## Performance

Benchmark on PLATEAU DEM GML (907 MB, 31M nodes) — [benchmark code](examples/load_test_cli.rs):

**Parse only:**

| Mode | Time | Throughput | Memory |
|------|------|------------|--------|
| libxml DOM | 4.39s | 207 MB/s | 2.93 GB |
| fastxml DOM | 5.21s | 174 MB/s | 1.73 GB |
| fastxml One-Pass | 4.20s | 216 MB/s | **~1 MB** |

**Parse + Schema Validation:**

| Mode | Time | Throughput | Memory |
|------|------|------------|--------|
| libxml DOM + validate | 9.88s | 92 MB/s | 2.93 GB |
| fastxml DOM + validate | 24.52s | 37 MB/s | 2.62 GB |
| fastxml Two-Pass | 29.56s | 31 MB/s | 780 MB |
| fastxml One-Pass | 62.01s | 15 MB/s | **~1 MB** |

- **DOM**: 1.7x less memory than libxml
- **Two-Pass**: Good balance of speed and memory
- **One-Pass**: Constant ~1 MB memory regardless of file size

## Installation

```toml
[dependencies]
fastxml = "0.3"
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
fastxml = { version = "0.3", features = ["ureq"] }

# Async schema fetching
fastxml = { version = "0.3", features = ["tokio"] }
```

### Schema Fetchers

| Fetcher | Description |
|---------|-------------|
| `FileFetcher` | Local filesystem |
| `UreqFetcher` | Sync HTTP (requires `ureq`) |
| `ReqwestFetcher` | Async HTTP (requires `tokio`) |
| `DefaultFetcher` | File + sync HTTP combined (requires `ureq` for HTTP) |
| `AsyncDefaultFetcher` | File + async HTTP combined (requires `tokio`) |

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
use fastxml::{parse, evaluate};

let xml = r#"<root><item id="1">Hello</item><item id="2">World</item></root>"#;

let doc = parse(xml.as_bytes())?;
let result = evaluate(&doc, "//item")?;
for node in result.into_nodes() {
    println!("{}: {}", node.get_attribute("id").unwrap(), node.get_content().unwrap());
}
```

### Streaming Parser

Process large files with minimal memory:

```rust
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use std::io::BufReader;
use std::fs::File;

struct Counter { count: usize }

impl XmlEventHandler for Counter {
    fn handle(&mut self, event: &XmlEvent) -> fastxml::error::Result<()> {
        if let XmlEvent::StartElement { .. } = event {
            self.count += 1;
        }
        Ok(())
    }
}

let file = File::open("large_file.xml")?;
let mut parser = StreamingParser::new(BufReader::new(file));
parser.add_handler(Box::new(Counter { count: 0 }));
parser.parse()?;
```

### Stream Transform

Transform XML with XPath-based element selection:

```rust
use fastxml::transform::StreamTransformer;

let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

// Modify elements
let result = StreamTransformer::new(xml)
    .xpath("//item[@id='2']")
    .transform(|node| node.set_attribute("modified", "true"))
    .to_string()?;

// Extract data
let ids: Vec<String> = StreamTransformer::new(xml)
    .xpath("//item")
    .collect(|node| node.get_attribute("id").unwrap_or_default())?;
```

## Async Schema Resolution

Parse XSD schemas with async import/include resolution (requires `tokio` feature):

```rust
use fastxml::schema::{
    AsyncDefaultFetcher, InMemoryStore,
    parse_xsd_with_imports_async,
};

#[tokio::main]
async fn main() -> fastxml::error::Result<()> {
    let xsd_content = std::fs::read("schema.xsd")?;

    // Create async fetcher and cache store
    let fetcher = AsyncDefaultFetcher::new()?;
    let store = InMemoryStore::new();

    // Parse schema with async import resolution
    let schema = parse_xsd_with_imports_async(
        &xsd_content,
        "http://example.com/schema.xsd",
        &fetcher,
        &store,
    ).await?;

    println!("Parsed {} types", schema.types.len());
    Ok(())
}
```

The async resolver:
- Fetches imported schemas asynchronously via HTTP
- Caches fetched schemas in the provided store
- Resolves nested imports (A → B → C)
- Detects circular dependencies

See [examples/async_schema_resolution.rs](examples/async_schema_resolution.rs) for more examples.

## Schema Validation

### DOM Validation

```rust
use fastxml::{parse, validate_document_by_schema};

let doc = parse(std::fs::read("document.xml")?.as_slice())?;
let errors = validate_document_by_schema(&doc, "schema.xsd".to_string())?;

if errors.is_empty() {
    println!("Valid!");
}
```

### One-Pass Validation

Validate during parsing with minimal memory:

```rust
use fastxml::schema::validator::OnePassSchemaValidator;
use std::sync::Arc;

let schema = Arc::new(fastxml::schema::parse_xsd(&std::fs::read("schema.xsd")?)?);
let reader = std::io::BufReader::new(file);

let errors = OnePassSchemaValidator::new(schema)
    .with_max_errors(100)
    .validate(reader)?;
```

### Two-Pass Validation

Balance of speed and memory for seekable streams:

```rust
use fastxml::schema::validator::TwoPassSchemaValidator;

let errors = TwoPassSchemaValidator::new(schema)
    .with_max_errors(100)
    .validate(reader)?;
```

### Validator Comparison

| Validator | Best For | Throughput | Memory |
|-----------|----------|------------|--------|
| `DomSchemaValidator` | Pre-parsed documents | ~37 MB/s | High |
| `TwoPassSchemaValidator` | Large seekable files | ~31 MB/s | Medium |
| `OnePassSchemaValidator` | Huge/non-seekable files | ~15 MB/s | **Minimal** |

### Auto-detect Schema

Fetch schemas from `xsi:schemaLocation` automatically (requires `ureq` feature):

```rust
use fastxml::{parse, validate_with_schema_location};

let doc = parse(xml_bytes)?;
let errors = validate_with_schema_location(&doc)?;
```

For streaming:

```rust
use fastxml::streaming_validate_with_schema_location;

let errors = streaming_validate_with_schema_location(reader)?;
```

### Async Validation

Validate with async schema fetching (requires `tokio` feature):

```rust
use fastxml::{parse, validate_with_schema_location_async};

#[tokio::main]
async fn main() -> fastxml::error::Result<()> {
    let doc = parse(xml_bytes)?;
    let errors = validate_with_schema_location_async(&doc).await?;
    Ok(())
}
```

Or get the compiled schema for reuse:

```rust
use fastxml::get_schema_from_schema_location_async;

let schema = get_schema_from_schema_location_async(&xml_bytes).await?;
```

### Validation Errors

```rust
use fastxml::ErrorLevel;

for error in &errors {
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
use fastxml::{parse, evaluate};

let doc = parse(xml)?;
let result = evaluate(&doc, "//item[@id='1']/text()")?;
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

let doc = parse(xml.as_bytes())?;
let buildings = evaluate(&doc, "//bldg:Building")?;
```

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
| Attribute/model groups | ✅ |
| import/include/redefine | ✅ |
| Built-in XSD and GML types | ✅ |
| Identity constraints (unique/key/keyref) | ✅ |
| Substitution groups | ✅ |

### Not Supported

- XQuery, XSLT, XInclude
- DTD validation
- XML Signature/Encryption
- Catalog support
- Full entity expansion

## Development

```bash
cargo test                              # Run tests
cargo test --features tokio             # With async tests
cargo test --features compare-libxml    # With libxml comparison
cargo bench                             # Benchmarks
```

### Examples

```bash
# Async schema resolution
cargo run --example async_schema_resolution --features tokio

# Schema validation
cargo run --example schema_validation --features ureq

# Load test CLI
cargo run --release --example load_test_cli -- ./file.xml
cargo run --release --features ureq --example load_test_cli -- ./file.xml --validate
```

## License

MIT OR Apache-2.0
