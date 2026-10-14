# Performance

Measurements are taken with [`examples/bench.rs`](../examples/bench.rs) on
PLATEAU CityGML files, release builds, best of several runs on one machine.
They compare fastxml with the libxml2 that ships with macOS (2.9.13).

```bash
cargo run --release --example bench -- ./file.xml                      # parse
cargo run --release --features ureq --example bench -- ./file.xml --validate
cargo run --release --example bench -- ./file.xml --mode streaming     # dom | streaming | both
```

## Current numbers

A 68 MB PLATEAU DEM file (`533521_dem_6697_op.gml`, 50 XSD schemas resolved),
measured in October 2026:

| | fastxml | libxml2 |
|---|---|---|
| DOM parse | 272 MiB/s | ~360 MiB/s |
| DOM memory (RSS delta of one parse) | 294–310 MB | — |
| Events from bytes | ~229 MiB/s | — |
| DOM validation of a parsed document | 0.51 s | — |
| Streaming parse + validation, from bytes | 0.50 s | ~0.38 s |
| Streaming parse + validation, from a reader | 0.58 s | — |

## Memory

- The streaming parser uses about 1.5 MB regardless of file size. Streaming
  validation adds the compiled schema and the collected errors, both bounded
  by schema size rather than document size.
- The DOM stores 128 bytes per node plus a compact interned attribute list.
  On text-heavy files this is far below libxml2: a 907 MB PLATEAU DEM took
  805 MB in fastxml and 4.19 GB in libxml2 (measured at v0.8.0).
- Errors collected during validation share interned strings, and
  `.aggregate_errors()` collapses identical errors, so error-dense documents
  stay memory-bounded.

## Where the time goes

- **Parsing** sits on quick-xml. The input is checked as UTF-8 once, start
  tags are decoded once, and namespace scopes are kept in an undo log.
- **Validation** interns names into integer symbols, memoizes per-type
  lookups, and matches children with a compiled content-model automaton.
  Unconstrained numeric values and lists (`gml:posList`,
  `gml:coordinates`) take an allocation-free scanner; any value it does not
  accept goes through the regular path, so error messages do not change.
- The remaining gap to libxml2 in streaming validation is spread over
  per-element bookkeeping, value checking and automaton stepping. The parser
  itself is the eventual ceiling.

## Comparing against libxml2

libxml2 cannot fetch the CityGML 2.0 schema set by itself: one import points
at an xAL schema URL that now answers with a redirect its fetcher does not
follow. The benchmark therefore resolves the set with fastxml, exports it with
a generated XML catalog (`fastxml::schema::export`) and hands that catalog to
libxml2 through `XML_CATALOG_FILES`, so both engines validate the same offline
schema set. The same catalog works with `xmllint --nonet`.
