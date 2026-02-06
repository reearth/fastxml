# fastxml-conformance

Conformance test suite for fastxml - W3C/OASIS standard compliance testing.

## Overview

This crate provides test harnesses for running standard XML test suites against fastxml:

| Test Suite | Tests | Target Functionality |
|-----------|-------|---------------------|
| W3C XML Conformance Test Suite | 2,378 | XML Parsing |
| W3C XML Schema Test Suite | 26,357 | XSD Validation |
| OASIS XPath 1.0 Test Suite | Hundreds | XPath Evaluation |

## Conformance Results (v0.8.0)

### W3C XML Conformance Test Suite

Tests both DOM parsing (`fastxml::parse`) and streaming parsing (`StreamingParser`).

| Category | DOM | Streaming | Description |
|----------|-----|-----------|-------------|
| valid | 89.9% (585/651) | 89.9% (585/651) | Documents that should parse successfully |
| invalid | 91.2% (207/227) | - | Documents valid as XML but invalid per DTD |
| not-wf | 20.9% (296/1415) | 20.5% (290/1415) | Malformed documents that should be rejected |

**Note**: The low not-wf pass rate is due to fastxml's lenient parsing of some malformed XML edge cases.

### W3C XML Schema Test Suite

Tests both DOM-based validation (`DomSchemaValidator`) and streaming validation (`OnePassSchemaValidator`).

| Category | DOM | Streaming |
|----------|-----|-----------|
| Schema compilation | 96.8% (14,981/15,480) | 96.8% (14,981/15,480) |
| Instance validation | 70.3% (18,462/26,260) | 69.9% (18,365/26,263) |

### Known Limitations

The following features are not fully supported, causing some test failures:

- **UTF-8 only**: UTF-16, ISO-8859-1, and other encodings are not supported
- **DTD entity expansion**: External and internal entity references are not expanded
- **Strict well-formedness**: Some malformed XML edge cases are accepted

## Quick Start

```bash
# Run tests (skips if data not available)
cargo test -p fastxml-conformance

# Download test data and run tests
FASTXML_DOWNLOAD_TESTS=1 cargo test -p fastxml-conformance

# Run specific test suite
cargo test -p fastxml-conformance w3c_xml
cargo test -p fastxml-conformance w3c_xsd
cargo test -p fastxml-conformance oasis_xpath
```

## Downloading Test Data

Test data is stored in `conformance/data/` (gitignored) and can be downloaded using:

```bash
# Download all test suites
cargo run -p fastxml-conformance --bin download

# Download specific suite
cargo run -p fastxml-conformance --bin download -- w3c-xml
```

Or set the environment variable to auto-download during tests:

```bash
FASTXML_DOWNLOAD_TESTS=1 cargo test -p fastxml-conformance
```

## Test Data Sources

- **W3C XML Conformance Test Suite**: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
- **W3C XML Schema Test Suite**: https://github.com/w3c/xsdtests

## Generating Reports

```bash
# Generate text report
cargo run -p fastxml-conformance --bin report

# Generate JSON report
cargo run -p fastxml-conformance --bin report -- --json
```

## Architecture

```
conformance/
├── src/
│   ├── lib.rs              # Common utilities and macros
│   ├── downloader.rs       # Test data download/extraction
│   ├── reporter.rs         # Conformance report generation
│   └── catalog/
│       ├── mod.rs
│       ├── xmlconf.rs      # W3C XML catalog parser
│       ├── xsdtests.rs     # W3C XSD catalog parser
│       └── oasis.rs        # OASIS XPath catalog parser
└── tests/
    ├── w3c_xml.rs          # W3C XML tests (DOM & Streaming)
    ├── w3c_xsd.rs          # W3C XSD tests (DOM & Streaming)
    └── oasis_xpath.rs      # OASIS XPath tests
```

## License

Same as fastxml: MIT OR Apache-2.0
