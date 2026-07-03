# fastxml-conformance

Conformance test suite for fastxml — W3C standard compliance testing.

## Overview

This crate runs standard XML test suites against fastxml and records **one
honest outcome per test**:

| Outcome | Meaning | Counts toward… |
|---------|---------|----------------|
| `pass` | fastxml produced the correct result | pass rate + coverage |
| `fail` | fastxml produced the wrong result | pass rate (as a miss) + coverage |
| `unsupported` | targets a feature fastxml deliberately omits (XML 1.1, 5th-edition-only rules, non-UTF-8 encodings, XSD 1.1) | neither |
| `blocked` | the harness could not decide (missing/unresolvable file, validator error before a verdict) | neither |
| `panic` | fastxml panicked | pass rate (as a miss) + coverage |

There is exactly one rate formula: **pass rate = `pass / (pass + fail + panic)`**
(decided tests only). **Coverage = `decided / total`** is always reported next
to it, so a high pass rate on a thin denominator is visible.

| Test Suite | Tests | Target |
|-----------|-------|--------|
| W3C XML Conformance Test Suite | 2,585 | XML parsing (DOM & streaming) |
| W3C XML Schema Test Suite | 39,613 | XSD validation (DOM & streaming) |

XPath is covered by unit tests only (`tests/xpath_basic.rs`); no external
standard XPath suite is run.

## Conformance Results (current `main`)

### W3C XML — 2,585 tests

| Category | DOM pass rate | Streaming pass rate | pass / fail / unsupported (DOM) |
|----------|---------------|---------------------|----------------------------------|
| valid                   | 93.2% | 93.2% | 383 / 28 / 401 |
| invalid (DTD)           | 96.7% | 96.7% | 208 / 7 / 27 |
| not-well-formed         | 26.2% | 25.7% | 319 / 897 / 282 |
| error (optional)        | —     | —     | 0 / 0 / 33 |
| **overall**             | **49.4%** | **49.0%** | **910 / 932 / 743** |

fastxml is a lenient, non-validating parser, so it accepts many malformed
documents (the low not-well-formed rate). `unsupported` is dominated by
XML 1.1 / 5th-edition tests and non-UTF-8 encodings.

### W3C XSD — 39,613 tests

| Category | DOM pass rate | Streaming pass rate | pass / fail / blocked (DOM) |
|----------|---------------|---------------------|------------------------------|
| valid schemas accepted     | 100.0% | 100.0% | 11,139 / 0 / 0 |
| invalid schemas rejected   | 52.3%  | 52.3%  | 1,753 / 1,599 / 0 |
| valid instances            | 99.4%  | 99.0%  | 13,673 / 86 / 304 |
| invalid instances rejected | 96.9%  | 96.6%  | 10,581 / 339 / 111 |
| **overall**                | **94.8%** | **94.6%** | **37,146 / 2,024 / 415** |

Every valid schema compiles (zero false rejections), but only 52.3% of invalid
schemas are rejected — fastxml is permissive toward malformed schemas.

> **Numbers from v0.9.x and earlier are not directly comparable** — the harness
> previously counted *any* error as a pass for negative tests and dropped
> blocked instances from the denominators.

## Baselines

Every non-pass result is snapshotted per suite/engine in
[`baselines/`](baselines/) as a sorted TSV (`category  id  outcome  detail`,
with a `#counts` header). The tests diff a fresh run against the baseline:

- A test that got **worse** (including a `pass` that started failing) is a
  **regression** and fails the test.
- A test that got **better** also fails, asking you to regenerate the baseline.
- A change in the total test count is **count drift** and fails.

The `detail` column is informational and never compared.

```bash
# After an intentional behaviour change, regenerate and commit the baselines:
FASTXML_UPDATE_BASELINE=1 cargo test -p fastxml-conformance

# Inspect how each outcome is classified (audit histogram to stderr):
FASTXML_CONFORMANCE_AUDIT=1 cargo run --release -p fastxml-conformance --bin report
```

## Quick Start

```bash
# Run tests (skips if data not available)
cargo test -p fastxml-conformance

# Download test data and run tests
FASTXML_DOWNLOAD_TESTS=1 cargo test -p fastxml-conformance

# Run a specific suite (both engines)
cargo test -p fastxml-conformance --test w3c_xml
cargo test -p fastxml-conformance --test w3c_xsd
cargo test -p fastxml-conformance --test xpath_basic
```

The XSD suite validates tens of thousands of instances; use `--release` for a
tolerable runtime.

## Downloading Test Data

Test data is stored in `conformance/data/` (gitignored):

```bash
cargo run -p fastxml-conformance --bin download
```

## Test Data Sources

- **W3C XML Conformance Test Suite**: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
- **W3C XML Schema Test Suite**: https://github.com/w3c/xsdtests

## Generating Reports

```bash
# JSON report: per suite/engine counts, pass rate, coverage, and non-pass records
cargo run --release -p fastxml-conformance --bin report -- --json
```

## Architecture

```
conformance/
├── baselines/              # Committed per-suite/engine non-pass snapshots (TSV)
├── src/
│   ├── lib.rs              # Data-dir helpers, env flags
│   ├── outcome.rs          # Outcome model + error classification
│   ├── baseline.rs         # Baseline TSV read/write + diff ratchet
│   ├── reporter.rs         # OutcomeCounts-based reporting
│   ├── downloader.rs       # Test data download/extraction
│   ├── runner/
│   │   ├── mod.rs          # Engine, SuiteRun, gating, encoding sniffing, audit
│   │   ├── xml.rs          # W3C XML suite runner
│   │   └── xsd.rs          # W3C XSD suite runner
│   └── catalog/
│       ├── xmlconf.rs      # W3C XML catalog parser
│       └── xsdtests.rs     # W3C XSD catalog parser
└── tests/
    ├── w3c_xml.rs          # Thin wrapper: run + baseline diff (DOM & streaming)
    ├── w3c_xsd.rs          # Thin wrapper: run + baseline diff (DOM & streaming)
    └── xpath_basic.rs      # Local XPath evaluator unit tests
```

## License

Same as fastxml: MIT OR Apache-2.0
