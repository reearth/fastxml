# fastxml

[![CI](https://github.com/reearth/fastxml/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/fastxml/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fastxml.svg)](https://crates.io/crates/fastxml)
[![docs.rs](https://docs.rs/fastxml/badge.svg)](https://docs.rs/fastxml)
[![License](https://img.shields.io/crates/l/fastxml.svg)](LICENSE)

A fast, memory-efficient XML library for Rust with XPath and schema validation support. Designed for processing large XML documents like CityGML files used in [PLATEAU](https://www.mlit.go.jp/plateau/).

## Features

- 🦀 **Pure Rust** — No C dependencies
- 💾 **Memory efficient** — Stream gigabyte-scale XML in about 1.5 MB; streaming validation does not grow with document size
- 🔍 **XPath 1.0** — Full XPath 1.0 with namespaces, plus streaming transforms selected by XPath
- 📋 **XSD validation** — XSD 1.0 with import/include/redefine and built-in GML types, on a DOM or while streaming
- 🔄 **libxml compatible** — Parsing and XPath results checked against libxml2
- ⚡ **Async** — Async schema fetching and resolution with tokio

> ⚠️ **Early development (v0.x)**: the API may change, and production experience is limited. Not recommended for business-critical systems.

## Installation

```toml
[dependencies]
fastxml = "0.12"
```

| Feature | Description |
|---------|-------------|
| `ureq` | Sync HTTP client for schema fetching (recommended) |
| `tokio` | Async HTTP client for schema fetching (reqwest + tokio) |
| `async-trait` | Async trait support for custom implementations |
| `compare-libxml` | Enable libxml2 comparison tests |

```toml
fastxml = { version = "0.12", features = ["ureq"] }   # sync schema fetching
fastxml = { version = "0.12", features = ["tokio"] }  # async schema fetching
```

## Quick start

Parse a document and query it with XPath:

```rust
use fastxml::{Parser, QueryExt};

let xml = r#"<root><item id="1">Hello</item><item id="2">World</item></root>"#;

let doc = Parser::from(xml).parse()?;
for node in doc.query_nodes("//item")? {
    println!("{}: {}", node.get_attribute("id").unwrap(), node.get_content().unwrap());
}
```

Read a large file as a stream of events in constant memory:

```rust
use fastxml::{Parser, event::XmlEvent};

let mut elements = 0;
Parser::from_reader(std::io::BufReader::new(file)).for_each_event(|event| {
    if let XmlEvent::StartElement { .. } = event {
        elements += 1;
    }
    Ok(())
})?;
```

Rewrite the elements an XPath selects, in a single streaming pass:

```rust
use fastxml::transform::Transformer;

let result = Transformer::from(xml)
    .on("//item[@id='2']", |node| node.set_attribute("modified", "true"))
    .to_string()?;
```

Validate against an XSD schema, on a DOM or while streaming:

```rust
use fastxml::schema::{Schema, Validator};

let schema = Schema::from_xsd(std::fs::read("schema.xsd")?)?;
let report = Validator::from_reader(reader).schema(schema).run()?;

for error in report.errors() {
    println!("{error}");
}
```

Leave out `.schema(..)` to load the schema named by the document's
`xsi:schemaLocation`.

## Documentation

| Guide | Contents |
|-------|----------|
| [Parsing and serializing](docs/parsing.md) | DOM parsing, streaming events, `Printer`, libxml compatibility functions, what is not supported |
| [XPath](docs/xpath.md) | Queries, namespaces, compiled `Query`, supported XPath 1.0 features |
| [Stream transform](docs/transform.md) | `Transformer`, reader-based transforms, read-only matches and early stopping, streamable XPath |
| [Schemas and validation](docs/validation.md) | Building schemas, DOM and streaming validation, schema auto-detection and its security, fetchers, the `fastxml-validate` CLI |
| [Performance](docs/performance.md) | Benchmarks against libxml2, memory use |
| [Conformance](docs/conformance.md) | W3C XML and XSD test suite results |

API reference: [docs.rs/fastxml](https://docs.rs/fastxml). Runnable examples are in [`examples/`](examples/).

## Performance

On a 68 MB PLATEAU DEM file (October 2026):

| | fastxml | libxml2 2.9.13 |
|---|---|---|
| DOM parse | 272 MiB/s | ~360 MiB/s |
| Streaming parse + validation | 0.50 s | ~0.38 s |

The streaming parser runs in about 1.5 MB of memory regardless of file size.
See [Performance](docs/performance.md) for the full numbers.

## Conformance

| Suite | Tests | Pass rate (DOM) |
|-------|------:|----------------:|
| W3C XML | 2,585 | 93.4% |
| W3C XSD | 39,613 | 98.7% |

Every valid schema in the XSD suite compiles, and 92.3% of the invalid ones
are rejected. See [Conformance](docs/conformance.md) for the categories and
how the rates are counted.

## Roadmap

- **XSD**: rules that span schemas brought in by import or include, and
  redefine legality. XSD 1.1 is not planned; fastxml targets XSD 1.0 and
  also accepts the XSD 1.1 datatypes.
- **XML parsing**: the not-well-formed cases that need entity expansion and
  re-parsing, or normalization before namespace URIs are compared.
- **XPath**: a differential test corpus against libxml2.
- **Performance**: closing the remaining gap to libxml2 in parsing and
  streaming validation.

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

See [AGENTS.md](AGENTS.md) for the repository layout and contribution checks.

## License

MIT OR Apache-2.0
