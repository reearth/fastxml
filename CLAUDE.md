# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

fastxml is a fast, memory-efficient XML library for Rust with XPath 1.0 and XSD schema validation support. Designed for processing large XML documents like CityGML files (PLATEAU).

## Build Commands

```bash
# Run tests
cargo test                              # Unit tests only
cargo test --lib --all-features         # All lib tests with all features
cargo test --features tokio             # With async tests
cargo test --features compare-libxml    # With libxml comparison (requires libxml2-dev)

# Run a single test
cargo test test_name                    # By test name
cargo test xpath_test                   # By test file name

# Linting
cargo fmt --all -- --check              # Check formatting
cargo clippy --all-targets --all-features -- -D warnings

# Build documentation
cargo doc --no-deps --all-features

# Run benchmarks
cargo bench

# Run examples
cargo run --example async_schema_resolution --features tokio
cargo run --example schema_validation --features ureq
cargo run --release --example bench -- ./file.xml
```

## Pre-commit Checklist

Before committing, always run all CI checks:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --all-features
cargo doc --no-deps --all-features
```

Note: `clippy` and `doc` with `--all-features` require `libxml2-dev` system package.

## Version Bump Checklist

When bumping the version, update these files:

- `Cargo.toml` - `version = "x.y.z"`
- `Cargo.lock` - Run `cargo update -p fastxml` to update the lock file
- `README.md` - All `fastxml = "x.y"` references in installation examples

## Manual Testing Tools

```bash
# Validate arbitrary XML files against XSD schema
cargo run --release --features ureq --bin fastxml-validate -- <xml-file>

# Run benchmarks on XML files
cargo run --release --example bench -- <xml-file>
cargo run --release --features ureq --example bench -- <xml-file> --validate

# Benchmark modes: --mode dom | streaming | both (default: both)
cargo run --release --example bench -- <xml-file> --mode streaming
```

## Architecture

### Core Modules (src/)

- **parser.rs** - XML parsing using quick-xml, produces DOM or events
- **document.rs** - DOM document structure (XmlDocument)
- **node.rs** - Node types (XmlNode for mutable, XmlRoNode for read-only)
- **event.rs** - Streaming parser with event handlers (XmlEvent, StreamingParser)
- **transform/** - Stream transformation with XPath-based element selection (StreamTransformer)

### XPath (src/xpath/)

- **parser.rs** - XPath 1.0 expression parser
- **evaluator.rs** - XPath evaluation engine
- **context.rs** - Evaluation context with namespace bindings (XmlContext, XmlSafeContext)
- **functions/** - Built-in XPath functions (string, number, nodeset, boolean)
- **axes.rs** - XPath axes implementation

### Schema/Validation (src/schema/)

- **xsd/** - XSD schema parsing and compilation
  - **parser/** - SAX-style XSD parser with stack-based state machine
  - **compiler.rs** - Compiles parsed XSD into CompiledSchema
  - **builtin.rs** - Built-in XSD and GML types
  - **resolver/** - Import/include resolution (sync and async)
- **validator/** - Validation implementations
  - **streaming.rs** - Single-pass streaming validator (OnePassSchemaValidator)
  - **lazy.rs** - Lazy schema loading validator
  - **dom.rs** - DOM-based validation
- **fetcher/** - Schema fetching (file, HTTP sync/async)
- **store.rs** - Schema storage abstraction (memory, temp files)

### Event Flow for Streaming Validation

```
XML Data → StreamingParser → [DocumentBuilder, OnePassSchemaValidator]
                               (same events shared)
```

## Key Types

- `XmlDocument` - Parsed XML document (DOM)
- `XmlNode` / `XmlRoNode` - Mutable / read-only node handles
- `CompiledSchema` - Compiled XSD schema for validation
- `StreamingParser` - Event-based parser for large files
- `StreamTransformer` - XPath-based stream transformation

## Feature Flags

- `ureq` - Sync HTTP client for schema fetching (enables CLI binary)
- `tokio` - Async HTTP client with reqwest
- `async-trait` - Async trait support for custom fetchers
- `compare-libxml` - Enable libxml2 comparison tests (requires system libxml2)
- `profile` - Memory profiling utilities

## Testing Notes

- Integration tests in `tests/` cover XSD validation, XPath, transforms
- `tests/common/mod.rs` contains shared test utilities
- libxml comparison tests require `libxml2-dev` system package

## Development Guidelines

### TDD (Test-Driven Development)

When fixing bugs or adding features, follow the TDD approach:

1. **Write a failing test first** - Create a test that reproduces the bug or specifies the new behavior
2. **Verify the test fails** - Run the test to confirm it fails as expected
3. **Implement the fix/feature** - Write the minimum code to make the test pass
4. **Verify the test passes** - Run the test to confirm the fix works
5. **Refactor if needed** - Clean up the code while keeping tests green

### File Size Guidelines

Keep individual `.rs` files under **1000 lines**. When a file grows beyond this threshold:

- Split it into submodules (e.g., `foo.rs` → `foo/mod.rs` + `foo/bar.rs`)
- Group related functionality together
- Use `pub use` in `mod.rs` to maintain the public API
