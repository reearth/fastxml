# Conformance

Conformance results on current `main`, measured by a harness that records one
honest outcome per test: `pass`, `fail`, `unsupported` (a feature fastxml
deliberately does not implement — XML 1.1, XML 1.0 5th-edition-only rules,
non-UTF-8 encodings), `blocked` (the harness could not decide, e.g. an
unresolvable schema import), or `panic`. The **pass rate is
`pass / (pass + fail + panic)`** — decided tests only — and **coverage** is the
share of all tests that were decided. Both engines (DOM and streaming) are run;
results are diffed against committed [baselines](../conformance/baselines/) so any
change in library behaviour must land with an updated baseline. See
[conformance/](../conformance/) for details.

## W3C XML

2,585 tests, DOM engine.

| Category | Pass rate | pass | fail | unsupported |
|----------|-----------|------|------|-------------|
| valid documents          | 93.4% | 382  | 27  | 403 |
| invalid documents (DTD)  | 96.7% | 206  | 7   | 29  |
| not-well-formed          | 92.8% | 1128 | 88  | 282 |
| error (optional)         | —     | 0    | 0   | 33  |
| **overall**              | **93.4%** | **1716** | **122** | **747** |

The streaming engine is within a few tests of DOM (overall 93.3%; not-well-formed
1126 pass / 90 fail). `unsupported` is dominated by XML 1.1 / 5th-edition-only
tests and non-UTF-8 encodings, which fastxml does not target (it targets XML 1.0
4th edition, UTF-8). fastxml is always namespace-aware, so the four
`NAMESPACE="no"` tests that assume a bare colon is an ordinary name character are
scored `unsupported` rather than run. Well-formedness enforcement covers the
`Char` and `Name` productions, document structure (single root, prolog/epilog
content, `]]>` in character data, unclosed elements), the full `DOCTYPE` internal
subset (PI targets and `ELEMENT` / `ATTLIST` / `ENTITY` / `NOTATION` declaration
grammar, conditional sections, `PubidLiteral` characters), character-reference
and entity semantics (malformed/illegal references, reference cycles), the XML
declaration, and the Namespaces in XML 1.0 constraints: `QName` syntax, prefix
binding, the reserved `xml` / `xmlns` namespace rules, expanded-name attribute
uniqueness, and NCName restrictions on PI targets and entity/notation names. The
remaining not-well-formed failures need character-reference / entity-reference /
attribute-value normalization before comparing namespace URIs (which the raw
per-event checker cannot do), plus constructs that require full entity expansion
and re-parsing.

## W3C XSD

39,613 tests, DOM engine.

| Category | Pass rate | pass | fail | blocked |
|----------|-----------|------|------|---------|
| valid schemas accepted      | 100.0% | 11,139 | 0   | 0   |
| invalid schemas rejected    | 92.3%  | 3,093  | 259 | 0   |
| valid instances             | 99.6%  | 13,877 | 54  | 132 |
| invalid instances rejected  | 98.1%  | 10,772 | 208 | 51  |
| **overall**                 | **98.7%** | **38,881** | **521** | **183** |

Schema compilation stays asymmetric by design: every valid schema compiles
(zero false rejections), while 92.3% of invalid schemas are rejected. The
rejection rules cover reference integrity (dangling QName references into
fully-present namespaces), circular definitions, cos-all-limited,
identity-constraint XPath grammar, attribute/placement/lexical
schema-for-schemas rules, derivation `final` controls, compile-time facet
validity, and a particle-restriction (rcase-*) engine — each rule only fires
when its verdict is certain, so partially-resolved real-world schema sets
(CityGML/PLATEAU) keep compiling. `blocked` instances are those whose schema
could not be resolved/compiled, plus one wildcard instance whose validity is
nondeterministic in fastxml. The streaming engine scores 98.7% overall
(38,880 pass). XSD 1.1-only test groups are excluded (XSD 1.0 target); the 28
indeterminate schema/instance tests are reported as `unsupported`.

The invalid-schema rules cover reference integrity, circular definitions,
cos-all-limited, identity-constraint XPath grammar, the attributes and the
order and required content of each XSD element, derivation controls, facet
validity (including against the facets of user-defined base types in the
same document), value constraints checked against every facet, attribute uses
under restriction, XSD regular-expression syntax, and a particle-restriction
(rcase-*) engine. The remaining invalid schemas mostly need rules that span
documents brought in by import or include, or redefine legality. `gMonth`
facet values in the pre-errata `--MM--` form stay accepted, because the suite
also has instances that use that form as valid.

> **Numbers from v0.9.x and earlier are not directly comparable** — the harness
> previously counted *any* error as a pass for negative tests and dropped
> blocked instances from the denominators, so both pass rates and totals were
> measured against the wrong basis.

## Running the suites

```bash
# Run conformance tests (requires test data download)
cargo run -p fastxml-conformance --bin download
cargo test -p fastxml-conformance

# Regenerate the committed baselines after an intentional behaviour change
FASTXML_UPDATE_BASELINE=1 cargo test -p fastxml-conformance
```
