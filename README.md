# fastxml

[![CI](https://github.com/reearth/fastxml/actions/workflows/ci.yml/badge.svg)](https://github.com/reearth/fastxml/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/fastxml.svg)](https://crates.io/crates/fastxml)
[![docs.rs](https://docs.rs/fastxml/badge.svg)](https://docs.rs/fastxml)
[![License](https://img.shields.io/crates/l/fastxml.svg)](LICENSE)

A fast, memory-efficient XML library for Rust with XPath and streaming schema validation support. Designed for processing large XML documents like CityGML files used in PLATEAU.

## Features

- **Streaming Parser**: Process gigabyte-scale XML with minimal memory footprint (~1-2 MB for multi-GB files)
- **DOM Parser**: Full document tree for random access and XPath queries
- **XPath Evaluation**: Support for common XPath expressions including namespaces
- **XSD Parser**: Full XSD schema parsing with import/include resolution
- **Schema Validation**: Streaming XSD validation with SAX event sharing
- **Built-in Types**: Pre-defined XSD primitives and GML types (CodeType, MeasureType, geometries)
- **CityGML Ready**: Optimized for PLATEAU/CityGML document processing

## Performance

### PLATEAU CityGML Benchmark (10.5 GB, 23 files)

| Metric | DOM | Streaming | Improvement |
|--------|-----|-----------|-------------|
| Parse + Validate | 1,008s | 548s | **1.84x faster** |
| Parse Throughput | 25.51 MB/s | 25.63 MB/s | - |
| Validate Throughput | 10.38 MB/s | 19.10 MB/s | **1.84x faster** |
| Peak Memory | 2,635 MB | 231 MB | **11.4x less** |

### Throughput

| Pattern | DOM Parse | Streaming | XPath |
|---------|-----------|-----------|-------|
| Many Elements (100K) | 52 MB/s | 90 MB/s | - |
| Deep Nesting (500) | 99 MB/s | 154 MB/s | - |
| Large Content (1MB) | 3.4 GB/s | 3.5 GB/s | - |
| CityGML (1000 buildings) | 70 MB/s | 117 MB/s | 54 MB/s |

### Memory Efficiency

| XML Size | DOM Memory | Streaming Memory |
|----------|------------|------------------|
| 41 MB | +128 MB | +48 KB |
| 208 MB | - | +1.3 MB |
| 840 MB | - | +1.1 MB |
| 1.23 GB | - | +1.1 MB |
| **2.06 GB** | - | **+1.4 MB** |

Streaming uses **~11x less memory** than DOM for large files with schema validation.

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

### PLATEAU/CityGML Support

Built-in support for GML and CityGML types used in PLATEAU:

```rust
use fastxml::schema::xsd::{parse_xsd, create_builtin_schema};

// Create schema with pre-registered GML types
let schema = create_builtin_schema();

// Available built-in types include:
// - XSD primitives: xs:string, xs:integer, xs:double, xs:dateTime, etc.
// - GML types: gml:CodeType, gml:MeasureType, gml:LengthType
// - GML geometry: gml:PointType, gml:PolygonType, gml:MultiSurfaceType, gml:SolidType
// - GML features: gml:AbstractFeatureType, gml:AbstractGMLType

// Parse PLATEAU building schema
let building_xsd = r#"
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:gml="http://www.opengis.net/gml/3.2">
    <xs:complexType name="BuildingType">
        <xs:sequence>
            <xs:element name="class" type="gml:CodeType" minOccurs="0"/>
            <xs:element name="measuredHeight" type="gml:LengthType" minOccurs="0"/>
            <xs:element name="storeysAboveGround" type="xs:nonNegativeInteger" minOccurs="0"/>
        </xs:sequence>
    </xs:complexType>
</xs:schema>
"#;

let schema = parse_xsd(building_xsd.as_bytes())?;
assert!(schema.types.contains_key("BuildingType"));
```

## Load Testing

### Run Benchmarks

```bash
cargo bench --bench load_test
```

### CLI Load Test Tool

Supports both synthetic data generation and real file benchmarking.

#### Synthetic Data (Pattern Mode)

```bash
# Many elements pattern
cargo run --release --example load_test_cli -- \
    --pattern many-elements --size 100000

# CityGML pattern (simulates PLATEAU data)
cargo run --release --example load_test_cli -- \
    --pattern citygml --size 50000 --mode streaming

# Deep nesting
cargo run --release --example load_test_cli -- \
    --pattern deep-nesting --size 500

# Large content per element
cargo run --release --example load_test_cli -- \
    --pattern large-content --size 1000
```

#### Real Files (Local or URL)

```bash
# Local files
cargo run --release --example load_test_cli -- ./file1.xml ./file2.xml

# URLs (requires sync feature)
cargo run --release --features ureq --example load_test_cli -- \
    https://example.com/citygml/file.xml

# From stdin (one URL/path per line)
cat urls.txt | cargo run --release --features ureq --example load_test_cli

# With schema validation
cargo run --release --example load_test_cli -- --validate ./document.xml
```

#### Options

| Option | Description |
|--------|-------------|
| `--pattern <PATTERN>` | Synthetic data: `many-elements`, `deep-nesting`, `large-content`, `citygml` |
| `--size <SIZE>` | Size for pattern (element count, depth, KB, or building count) |
| `--mode <MODE>` | Processing mode: `dom`, `streaming`, or `both` (default) |
| `--iterations <N>` | Number of iterations (default: 3) |
| `--validate` | Enable schema validation benchmark |
| `--cache-dir <DIR>` | Cache directory for downloaded URLs (default: `benches/cache`) |

## API Reference

### Core Functions

```rust
// Parse XML into DOM
fn parse<T: AsRef<[u8]>>(xml: T) -> Result<XmlDocument>

// Evaluate XPath expression
fn evaluate(doc: &XmlDocument, xpath: &str) -> Result<XPathResult>

// Get root node
fn get_root_node(doc: &XmlDocument) -> Result<XmlNode>

// Serialize node to string
fn node_to_xml_string(doc: &XmlDocument, node: &mut XmlNode) -> Result<String>
```

### Streaming API

```rust
// Create streaming parser
let parser = StreamingParser::new(reader);

// Add event handlers
parser.add_handler(Box::new(handler));

// Parse document
parser.parse()?;
```

### Schema Validation

```rust
// Parse XSD schema and create validation context
let xsd_content = std::fs::read("schema.xsd")?;
let schema = fastxml::parse_xsd(&xsd_content)?;

// Create validation context
let ctx = create_xml_schema_validation_context_from_buffer(&xsd_content)?;

// Validate document
let errors = validate_document_by_schema(&doc, schema_location)?;

// Streaming validation
let validator = StreamingSchemaValidator::new(Arc::new(schema));
parser.add_handler(Box::new(validator));
```

### XSD Parsing with Import Resolution

```rust
use fastxml::schema::{parse_xsd_with_imports, UreqFetcher, TempDirStore};

// Create fetcher and store for resolving imports
let fetcher = UreqFetcher::new();
let store = TempDirStore::new()?;

// Parse XSD with all dependencies resolved
let schema = parse_xsd_with_imports(
    xsd_content,
    "https://example.com/schema.xsd",
    &fetcher,
    &store,
)?;
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

**Not supported:**

| Feature | Status |
|---------|--------|
| Union operator (`\|`) | ❌ |
| Namespace axis | ❌ |
| Variables (`$var`) | ❌ |

### XSD Schema

**Supported:** Element/attribute definitions, complex types (sequence/choice/all), simple types (restriction/list/union), type inheritance, facets, attribute/model groups, import/include/redefine, built-in XSD and GML types, identity constraints (unique/key/keyref), streaming validation with error location info.

**Partial:** Substitution groups (parsing only).

### Not Supported

- XQuery, DTD validation, XSLT, XInclude, XML Signature/Encryption
- Catalog support
- DOM modification (read-only)
- Entity expansion (basic only)

## License

MIT OR Apache-2.0
