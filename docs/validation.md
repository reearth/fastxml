# Schemas and validation

fastxml compiles XSD 1.0 schemas (with import, include and redefine) and
validates documents against them, either on a parsed DOM or while streaming.

- [Building a schema](#building-a-schema)
- [Validating](#validating)
- [Schema fetchers](#schema-fetchers)
- [Content-model query](#content-model-query)
- [Supported XSD features](#supported-xsd-features)
- [Command-line validator](#command-line-validator)

## Building a schema

A `Schema` is built with `Schema::from_xsd(bytes)`, `Schema::builtin()` (the
built-in XSD and GML types only), or `Schema::builder()`.
`Schema::builder()` takes one or more `.add(uri, bytes)` sources; finish with
`.resolve()` (no network), `.resolve_with(&fetcher)`, or
`.resolve_with_async(&fetcher)`.

```rust
use fastxml::schema::{DefaultFetcher, Schema};

let schema = Schema::builder()
    .add("https://example.com/schema.xsd", std::fs::read("schema.xsd")?)
    .resolve_with(&DefaultFetcher::new())?;
```

### Resolving imports asynchronously

With the `tokio` feature, imports and includes can be fetched asynchronously:

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

The async resolver:
- Fetches imported schemas asynchronously via HTTP
- Resolves nested imports (A → B → C)
- Detects circular dependencies

See [examples/async_schema_resolution.rs](../examples/async_schema_resolution.rs) for more examples.

## Validating

All validation goes through one `Validator` front door: the input type selects
the engine (`&XmlDocument` → DOM, `&str`/`&[u8]`/reader → streaming),
`.schema(..)` supplies an explicit schema (or it is resolved from
`xsi:schemaLocation`), and `run()` returns a `Report`.

### DOM validation

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

### Streaming validation

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

### Auto-detecting the schema

Omit `.schema(..)` and the schema is resolved from the root element's `xsi:schemaLocation` / `xsi:noNamespaceSchemaLocation` (any prefix bound to the XML Schema instance namespace), using the default fetcher (requires the `ureq` feature):

```rust
use fastxml::{Parser, schema::Validator};

let doc = Parser::from(xml_bytes).parse()?;
let report = Validator::from(&doc).run()?;
```

For streaming input, the root start tag is read first to find the hints, the
schema is loaded, and then the whole input is validated in one pass (the bytes
read ahead are replayed, so nothing is read twice from the source):

```rust
use fastxml::schema::Validator;

let report = Validator::from_reader(reader).run()?;
```

To supply a custom fetcher, use `.run_with(fetcher)` instead of `.run()`.

If the document names no schema, or a named schema (or anything it imports or includes) cannot be fetched, parsed or compiled, the report carries an error-level `SchemaNotFound` entry explaining what failed, the content is not validated, and `is_valid()` is `false`. Relative locations are resolved by the fetcher (e.g. `DefaultFetcher::with_base_dir(dir)`).

#### Security: untrusted documents

Auto-detection fetches whatever the document names: its `xsi:schemaLocation` / `xsi:noNamespaceSchemaLocation`, and then every `xs:import` / `xs:include` / `xs:redefine` location in the fetched schemas. With the default fetcher that means any local file path or `file://` URL is read and any URL is requested. When the document comes from an untrusted source, do one of the following:

- Pass an explicit schema with `.schema(..)`, so nothing named by the document is fetched.
- Use `NoopFetcher` (`.run_with(NoopFetcher)`) together with a schema you supply.
- Wrap the fetcher in `RestrictedFetcher` to allow only listed URL prefixes and local directories:

```rust
use fastxml::schema::{DefaultFetcher, RestrictedFetcher, Validator};

let fetcher = RestrictedFetcher::new(DefaultFetcher::new())
    .allow_url_prefix("https://schemas.opengis.net/")
    .allow_dir("./schemas");
let report = Validator::from(&doc).run_with(fetcher)?;
```

A URL must match an allowed prefix by scheme, host, port and path (after normalizing `..` segments). A local path or `file://` URL must lie inside an allowed directory after resolving `..` and symlinks. Any other location fails to load and the report is invalid. The final URL after an HTTP redirect is checked too, but by then the redirected request has already been sent. To avoid contacting the redirect target at all, use an inner fetcher that does not follow redirects, such as `UreqFetcher::new().max_redirects(0)`.

The `fastxml-validate` CLI does the same with the repeatable `--allow-url <PREFIX>` and `--allow-dir <DIR>` options.

### Async validation

Validate with async schema fetching (requires `tokio` feature) via `run_async()` (default fetcher) or `run_async_with(&fetcher)`. This works for DOM and streaming input alike; only the schema fetching is async, the input is read synchronously:

```rust
use fastxml::{Parser, schema::Validator};

#[tokio::main]
async fn main() -> fastxml::error::Result<()> {
    let doc = Parser::from(xml_bytes).parse()?;
    let report = Validator::from(&doc).run_async().await?;
    Ok(())
}
```

### Reading the report

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

### Aggregated errors

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

## Schema fetchers

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
| `RestrictedFetcher` | Wraps any sync fetcher; only fetches allowed URL prefixes and local directories |
| `AsyncRestrictedFetcher` | Wraps any async fetcher; only fetches allowed URL prefixes and local directories (requires `tokio`) |
| `NoopFetcher` | Fetches nothing (every fetch fails) |

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

## Content-model query

A producer that writes XML can ask a compiled schema where each child element
goes in its parent's content model, so children are emitted in the order the
schema declares, including ADE elements that arrive through substitution groups
(e.g. PLATEAU i-UR properties at CityGML's `_GenericApplicationPropertyOf…`
hooks). Matching is namespace-exact.

```rust
use fastxml::schema::Schema;

let schema = Schema::builder().add("building.xsd", xsd_bytes).resolve()?;
let bldg = "http://www.opengis.net/citygml/building/2.0";
let uro = "https://www.geospatial.jp/iur/uro/3.2";

// By type name, or by global element name (`element_content_automaton`).
let model = schema.type_content_automaton(bldg, "BuildingType").unwrap();
let height = model.position_of(Some(bldg), "measuredHeight"); // Some(index)
let ade = model.position_of(Some(uro), "buildingIDAttribute"); // the ADE hook's index
assert!(height < ade);
```

## Supported XSD features

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
| Other XSD 1.1 features (assertions, conditional type assignment, openContent) | ❌ |
| xs:override (XSD 1.1) | ❌ (recorded in the AST; compiling a schema that uses it is an error) |

How strictly invalid schemas are rejected is measured against the W3C test
suite; see [Conformance](conformance.md#w3c-xsd).

## Command-line validator

The `fastxml-validate` binary (feature `ureq`) validates files against the
schemas they name, or against one given with `--schema`:

```bash
cargo run --release --features ureq --bin fastxml-validate -- ./file.xml
cargo run --release --features ureq --bin fastxml-validate -- --schema schema.xsd a.xml b.xml
```

`--allow-url <PREFIX>` and `--allow-dir <DIR>` (both repeatable) restrict which
schema locations are fetched, as `RestrictedFetcher` does. `--json` prints the
results as JSON. The exit code is 0 when every file is valid, 1 when at least
one is invalid, and 2 when the run could not complete.
