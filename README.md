# fastxml

[![CI](https://github.com/reearth/fastxml/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/fastxml/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fastxml.svg)](https://crates.io/crates/fastxml)
[![docs.rs](https://docs.rs/fastxml/badge.svg)](https://docs.rs/fastxml)
[![License](https://img.shields.io/crates/l/fastxml.svg)](LICENSE)

A fast, memory-efficient XML library for Rust with XPath and streaming schema validation support. Designed for processing large XML documents like CityGML files used in [PLATEAU](https://www.mlit.go.jp/plateau/).

## Features

- **Pure Rust**: No C dependencies, no unsafe code
- **libxml Compatible**: Tested against libxml2 to ensure consistent parsing and XPath results, with **3,600x better memory efficiency** in streaming mode
- **Streaming Parser**: Process gigabyte-scale XML with minimal memory footprint (~1-2 MB for multi-GB files)
- **DOM Parser**: Full document tree for random access and XPath queries
- **DOM Modification**: Mutable node API and streaming XPath-based transformation
- **XPath Evaluation**: Support for common XPath expressions including namespaces
- **XSD Parser**: Full XSD schema parsing with import/include resolution
- **Schema Validation**: Streaming XSD validation with SAX event sharing
- **Built-in Types**: Pre-defined XSD primitives and GML types (CodeType, MeasureType, geometries)
- **CityGML Ready**: Optimized for PLATEAU/CityGML document processing

## Performance

### Comparison with libxml

fastxml is designed as a drop-in replacement for libxml in Rust projects:

| Feature | libxml | fastxml |
|---------|--------|---------|
| DOM parsing | ✅ | ✅ |
| XPath | ✅ (full) | ✅ (subset) |
| Schema validation | ✅ | ✅ (streaming) |
| Streaming | ❌ | ✅ |
| Memory efficiency | Low | High |
| Pure Rust | ❌ | ✅ |

**Benchmark** (PLATEAU DEM GML, 907 MB, 31M nodes):

| Mode | Time | Throughput | Memory |
|------|------|------------|--------|
| libxml DOM | 3.41s | 266 MB/s | **3.78 GB** |
| fastxml DOM | 4.23s | 214 MB/s | 1.02 GB |
| fastxml Streaming | 3.21s | 282 MB/s | **1.05 MB** |

- **DOM**: fastxml uses **3.7x less memory** than libxml (trades ~20% speed)
- **Streaming**: fastxml uses **3,600x less memory** than libxml DOM, and is **faster**

fastxml's streaming mode is ideal for processing large XML files on memory-constrained systems.

**Compatibility Testing**: Parsing, XPath, and validation results are verified against libxml2. Run with `cargo test --features compare-libxml` (requires libxml2-dev).

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
fastxml = "0.1"
```

### Features

By default, no HTTP client is included. Choose the features you need:

| Feature | Description |
|---------|-------------|
| `ureq` | Sync HTTP client (`UreqFetcher`) for schema fetching |
| `reqwest` | Async HTTP client (`ReqwestFetcher`) for schema fetching |
| `async-trait` | Async trait support for custom `AsyncSchemaStore` implementations |
| `profile` | Memory profiling utilities |
| `compare-libxml` | Enable libxml2 comparison tests (requires libxml2-dev) |

```toml
# For sync schema fetching
fastxml = { version = "0.1", features = ["ureq"] }

# For async schema fetching
fastxml = { version = "0.1", features = ["reqwest"] }

# For custom async implementations (without built-in HTTP client)
fastxml = { version = "0.1", features = ["async-trait"] }
```

## Quick Start

### DOM Parsing

```rust
use fastxml::{parse, evaluate};

let xml = r#"
<root>
    <item id="1">Hello</item>
    <item id="2">World</item>
</root>
"#;

// Parse XML
let doc = parse(xml.as_bytes())?;
println!("Node count: {}", doc.node_count());

// XPath query
let result = evaluate(&doc, "//item")?;
for node in result.into_nodes() {
    println!("Found: {}", node.tag_name());
}
```

### Streaming Parser

Process large files with minimal memory:

```rust
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use std::io::BufReader;
use std::fs::File;

struct MyHandler {
    element_count: usize,
}

impl XmlEventHandler for MyHandler {
    fn handle(&mut self, event: &XmlEvent) -> fastxml::error::Result<()> {
        if let XmlEvent::StartElement { name, .. } = event {
            self.element_count += 1;
            println!("Element: {}", name);
        }
        Ok(())
    }
}

let file = File::open("large_file.xml")?;
let reader = BufReader::new(file);

let mut parser = StreamingParser::new(reader);
parser.add_handler(Box::new(MyHandler { element_count: 0 }));
parser.parse()?;
```

### Streaming Transform

Transform XML documents efficiently with XPath-based element selection. Only matched elements are converted to DOM, providing significant memory savings for large files.

```rust
use fastxml::transform::StreamTransformer;

let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;

// Modify specific elements
let result = StreamTransformer::new(xml)
    .xpath("//item[@id='2']")
    .transform(|node| {
        node.set_attribute("modified", "true");
    })
    .to_string()
    .unwrap();
// Result: <root><item id="1">A</item><item id="2" modified="true">B</item></root>

// Remove elements
let result = StreamTransformer::new(xml)
    .xpath("//item[@id='1']")
    .transform(|node| {
        node.remove();
    })
    .to_string()
    .unwrap();
// Result: <root><item id="2">B</item></root>

// Extract data without transformation
let ids: Vec<String> = StreamTransformer::new(xml)
    .xpath("//item")
    .collect(|node| node.get_attribute("id").unwrap_or_default())
    .unwrap();
// ids: ["1", "2"]

// Iterate over matched elements
let mut count = 0;
StreamTransformer::new(xml)
    .xpath("//item")
    .for_each(|node| {
        println!("Found: {:?}", node.get_content());
        count += 1;
    })
    .unwrap();
```

With namespace support:

```rust
use fastxml::{parse, transform::StreamTransformer};

let xml = r#"<root xmlns:gml="http://www.opengis.net/gml">
    <gml:Point><gml:pos>1 2</gml:pos></gml:Point>
</root>"#;

// Option 1: Register namespaces manually
let result = StreamTransformer::new(xml)
    .namespaces([
        ("gml", "http://www.opengis.net/gml"),
        ("bldg", "http://www.opengis.net/citygml/building/2.0"),
    ])
    .xpath("//gml:Point")
    .transform(|node| {
        node.set_attribute("srsName", "EPSG:4326");
    })
    .to_string()
    .unwrap();

// Option 2: Import namespaces from parsed document
let doc = parse(xml).unwrap();
let result = StreamTransformer::new(xml)
    .with_document_namespaces(&doc)
    .xpath("//gml:Point")
    .transform(|node| {
        node.set_attribute("srsName", "EPSG:4326");
    })
    .to_string()
    .unwrap();
```

**Performance** (100K elements, 11 MB XML):

| Approach | Time | Memory |
|----------|------|--------|
| Streaming Transform | 47ms | ~11 MB |
| DOM Parse + XPath | 141ms | ~135 MB |

Streaming is **3x faster** and uses **12x less memory**.

### Schema Validation

Validate XML documents against XSD schemas:

```rust
use fastxml::{parse, validate_document_by_schema};

// Parse the XML document
let xml = std::fs::read("document.xml")?;
let doc = parse(&xml)?;

// Validate against XSD schema (fetches imports automatically)
let errors = validate_document_by_schema(&doc, "schema.xsd".to_string())?;

if errors.is_empty() {
    println!("Document is valid!");
} else {
    for error in &errors {
        println!("{}", error);
    }
}
```

### Streaming Validation

For large files, validate while parsing in a single pass:

```rust
use fastxml::event::StreamingParser;
use fastxml::schema::validator::StreamingSchemaValidator;
use fastxml::schema::parse_xsd;
use std::sync::Arc;
use std::io::BufReader;
use std::fs::File;

// Load and compile the schema
let xsd_content = std::fs::read("schema.xsd")?;
let schema = Arc::new(parse_xsd(&xsd_content)?);

// Create streaming parser with validation
let file = File::open("large_document.xml")?;
let mut parser = StreamingParser::new(BufReader::new(file));

let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
parser.add_handler(Box::new(validator));

// Parse and validate in single pass
parser.parse()?;
```

### Error Handling

Validation errors include detailed location and context information:

```rust
use fastxml::{parse, validate_document_by_schema, ErrorLevel};

let doc = parse(xml_bytes)?;
let errors = validate_document_by_schema(&doc, schema_path)?;

for error in &errors {
    // Error severity: Warning, Error, or Fatal
    match error.level {
        ErrorLevel::Warning => print!("[WARN] "),
        ErrorLevel::Error => print!("[ERROR] "),
        ErrorLevel::Fatal => print!("[FATAL] "),
    }

    // Location information
    if let Some(path) = &error.element_path {
        print!("{}", path);
    }
    if let Some(line) = error.line {
        print!(" (line {})", line);
    }
    print!(": ");

    // Error message with expected/found values
    println!("{}", error.message);
    if let (Some(expected), Some(found)) = (&error.expected, &error.found) {
        println!("  expected: {}, found: {}", expected, found);
    }
}

// Filter by severity
let fatal_errors: Vec<_> = errors.iter()
    .filter(|e| e.level == ErrorLevel::Fatal)
    .collect();
```

### XPath with Namespaces

```rust
use fastxml::{parse, evaluate};

let xml = r#"
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
    <bldg:Building gml:id="bldg_001">
        <bldg:measuredHeight>25.5</bldg:measuredHeight>
    </bldg:Building>
</core:CityModel>
"#;

let doc = parse(xml.as_bytes())?;

// Query with namespace prefix
let buildings = evaluate(&doc, "//bldg:Building")?;
println!("Found {} buildings", buildings.into_nodes().len());

// Query with name() function
let heights = evaluate(&doc, "//*[name()='measuredHeight']/text()")?;
```

## Limitations

### XPath

**Supported expressions:**

| Expression | Example | Description |
|------------|---------|-------------|
| Absolute path | `/root/child` | Direct path from root |
| Descendant | `//element` | Any descendant |
| Wildcard | `//*` | All elements |
| Name predicate | `//*[name()='Building']` | Match by name |
| Logical operators | `//*[name()='A' or name()='B']` | `and`, `or`, `not` |
| Text | `//element/text()` | Text content |
| Namespace | `//bldg:Building` | Namespaced elements |
| Axes | `ancestor::div`, `following-sibling::*` | All standard axes |
| Arithmetic | `@value + 10` | `+`, `-`, `*`, `div`, `mod` |
| Comparison | `@count > 5` | `=`, `!=`, `<`, `>`, `<=`, `>=` |
| Functions | `count(//item)`, `contains(@name, 'test')` | Position, string, math functions |
| Union | `//a \| //b` | Combine multiple paths |
| Variables | `//item[@id=$target]` | Variable references |
| Namespace axis | `namespace::*` | In-scope namespaces |

### XSD Schema

**Supported:** Element/attribute definitions, complex types (sequence/choice/all), simple types (restriction/list/union), type inheritance, facets, attribute/model groups, import/include/redefine, built-in XSD and GML types, identity constraints (unique/key/keyref), streaming validation with error location info.

**Partial:** Substitution groups (parsing only).

### Not Supported

- XQuery, DTD validation, XSLT, XInclude, XML Signature/Encryption
- Catalog support
- Entity expansion (basic only)

## Development

```bash
cargo test                              # Run all tests
cargo test --features compare-libxml    # With libxml comparison (requires libxml2-dev)
cargo bench                             # Run benchmarks
```

### Load Test CLI

```bash
# Synthetic data
cargo run --release --example load_test_cli -- --pattern citygml --size 50000

# Real files
cargo run --release --example load_test_cli -- ./file.xml

# Compare with libxml
cargo run --release --features compare-libxml --example load_test_cli -- --mode dom ./file.xml
```

| Option | Description |
|--------|-------------|
| `--pattern <PATTERN>` | `many-elements`, `deep-nesting`, `large-content`, `citygml` |
| `--size <SIZE>` | Size for pattern |
| `--mode <MODE>` | `dom`, `streaming`, or `both` (default) |
| `--validate` | Enable schema validation |

## License

MIT OR Apache-2.0
