//! Characterization tests for streaming validation with the schema
//! auto-detected from the root element's hints.
//!
//! Each case pins the exact report (entry type, level, line, column and
//! message) or the exact parse error, for byte input and for reader input
//! read through a tiny buffer. The streaming engine reads the prolog and
//! root start tag before the schema is loaded; these tests make sure the
//! entries, and their positions, stay what they were.

use std::collections::HashMap;
use std::io::BufReader;

use fastxml::schema::fetcher::error::FetchError;
use fastxml::schema::{FetchResult, SchemaFetcher, Validator};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

const SCHEMA_T: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns="urn:t" xmlns:u="urn:u" elementFormDefault="qualified">
  <xs:import namespace="urn:u"/>
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
      <xs:element ref="u:extra" minOccurs="0"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

const SCHEMA_U: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:u" elementFormDefault="qualified">
  <xs:element name="extra" type="xs:boolean"/>
</xs:schema>"#;

const SCHEMA_NONS: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

/// In-memory fetcher: `mem://t.xsd`, `mem://u.xsd`, `mem://nons.xsd`.
#[derive(Clone)]
struct MemFetcher(HashMap<String, Vec<u8>>);

impl MemFetcher {
    fn new() -> Self {
        let mut m = HashMap::new();
        m.insert("mem://t.xsd".to_string(), SCHEMA_T.as_bytes().to_vec());
        m.insert("mem://u.xsd".to_string(), SCHEMA_U.as_bytes().to_vec());
        m.insert(
            "mem://nons.xsd".to_string(),
            SCHEMA_NONS.as_bytes().to_vec(),
        );
        Self(m)
    }

    fn get(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        match self.0.get(url) {
            Some(content) => Ok(FetchResult {
                content: content.clone(),
                final_url: url.to_string(),
                redirected: false,
            }),
            None => Err(FetchError::RequestFailed {
                url: url.to_string(),
                message: "not in memory".to_string(),
            }
            .into()),
        }
    }
}

impl SchemaFetcher for MemFetcher {
    fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        self.get(url)
    }
}

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl fastxml::schema::AsyncSchemaFetcher for MemFetcher {
    async fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        self.get(url)
    }
}

/// One line per entry (`type|level|line:column|message`, plus `x<count>`
/// for an aggregated entry), or the error of the run.
fn describe(result: fastxml::error::Result<fastxml::schema::Report>) -> Vec<String> {
    match result {
        Ok(report) => report
            .entries()
            .iter()
            .map(|e| {
                let count = if e.count == 1 {
                    String::new()
                } else {
                    format!(" x{}", e.count)
                };
                format!(
                    "{:?}|{:?}|{:?}:{:?}|{}{count}",
                    e.error_type, e.level, e.location.line, e.location.column, e.message
                )
            })
            .collect(),
        Err(e) => vec![format!("Err({e})")],
    }
}

fn from_bytes(xml: &[u8]) -> Vec<String> {
    describe(Validator::from(xml).run_with(MemFetcher::new()))
}

fn from_reader(xml: &[u8]) -> Vec<String> {
    // A 3-byte buffer forces the input to arrive in many small reads.
    describe(Validator::from_reader(BufReader::with_capacity(3, xml)).run_with(MemFetcher::new()))
}

/// Runs byte and reader input (which must agree) and compares the outcome.
#[track_caller]
fn check(xml: &str, expected: &[&str]) {
    let bytes = from_bytes(xml.as_bytes());
    let reader = from_reader(xml.as_bytes());
    assert_eq!(bytes, reader, "byte and reader input disagree");
    assert_eq!(bytes, expected);
}

fn doc_t(body: &str) -> String {
    format!(
        "<root xmlns=\"urn:t\" xmlns:xsi=\"{XSI}\"\n      xsi:schemaLocation=\"urn:t mem://t.xsd\">{body}</root>"
    )
}

const NO_HINTS: &str = "SchemaNotFound|Error|None:None|no schema to validate against: the root \
element has no xsi:schemaLocation or xsi:noNamespaceSchemaLocation attribute (namespace \
http://www.w3.org/2001/XMLSchema-instance); the document content was not validated. Add a hint \
to the document or pass a schema explicitly with Validator::schema(..)";

fn fetch_failed(location: &str, named_by: &str) -> String {
    format!(
        "SchemaNotFound|Error|None:None|could not fetch schema '{location}' named by the \
         {named_by}: fetch error: failed to fetch {location}: not in memory; the document \
         content was not validated. Check that the location is reachable (a relative location \
         is resolved by the fetcher, against its base directory or else the current working \
         directory), or pass a schema explicitly with Validator::schema(..)"
    )
}

#[test]
fn valid_document_with_hint() {
    check(&doc_t("<n>1</n><n>2</n>"), &[]);
}

#[test]
fn invalid_content_reports_positions() {
    check(
        &doc_t("\n  <n>abc</n>\n  <n>2</n>\n    <bad/>\n"),
        &[
            "InvalidTextContent|Error|Some(3):Some(6)|element 'n': 'abc' is not a valid xs:int",
            "UnknownElement|Error|Some(5):Some(11)|element 'bad' is not declared in schema",
        ],
    );
}

#[test]
fn missing_schema() {
    let xml = format!(
        "<root xmlns=\"urn:t\" xmlns:xsi=\"{XSI}\" xsi:schemaLocation=\"urn:t mem://missing.xsd\"><n>abc</n></root>"
    );
    check(
        &xml,
        &[&fetch_failed(
            "mem://missing.xsd",
            "xsi:schemaLocation entry for namespace 'urn:t'",
        )],
    );
}

#[test]
fn missing_schema_still_checks_well_formedness() {
    let xml = format!(
        "<root xmlns=\"urn:t\" xmlns:xsi=\"{XSI}\" xsi:schemaLocation=\"urn:t mem://missing.xsd\"><n>abc</m></root>"
    );
    check(
        &xml,
        &[
            "Err(parse error: parse error at position 129: ill-formed document: expected `</n>`, but `</m>` was found)",
        ],
    );
}

#[test]
fn loaded_schema_then_malformed_content() {
    check(
        &doc_t("<n>abc</n><n>1</x>"),
        &[
            "Err(parse error: parse error at position 137: ill-formed document: expected `</n>`, but `</x>` was found)",
        ],
    );
}

#[test]
fn no_hint() {
    check(
        "<?xml version=\"1.0\"?>\n<root><n>1</n></root>",
        &[NO_HINTS],
    );
}

#[test]
fn malformed_hint() {
    let xml = format!("<root xmlns:xsi=\"{XSI}\" xsi:schemaLocation=\"urn:t\"/>");
    check(
        &xml,
        &[
            "SchemaNotFound|Error|None:None|xsi:schemaLocation must hold namespace/location pairs, but namespace 'urn:t' has no location; the document content was not validated",
        ],
    );
}

#[test]
fn multiple_hints() {
    let xml = format!(
        "<root xmlns=\"urn:t\" xmlns:u=\"urn:u\" xmlns:xsi=\"{XSI}\"\n  xsi:schemaLocation=\"urn:t mem://t.xsd\n                      urn:u mem://u.xsd\">\n  <n>1</n>\n  <u:extra>maybe</u:extra>\n</root>"
    );
    check(
        &xml,
        &[
            "InvalidTextContent|Error|Some(5):Some(12)|element 'u:extra': 'maybe' is not a valid xs:boolean",
        ],
    );
}

#[test]
fn multiple_hints_one_missing() {
    let xml = format!(
        "<root xmlns=\"urn:t\" xmlns:xsi=\"{XSI}\" xsi:schemaLocation=\"urn:t mem://t.xsd urn:u mem://nope.xsd\" xsi:noNamespaceSchemaLocation=\"mem://gone.xsd\"><n>1</n></root>"
    );
    check(
        &xml,
        &[
            &fetch_failed(
                "mem://nope.xsd",
                "xsi:schemaLocation entry for namespace 'urn:u'",
            ),
            &fetch_failed("mem://gone.xsd", "xsi:noNamespaceSchemaLocation"),
        ],
    );
}

#[test]
fn no_namespace_hint() {
    let xml = format!(
        "<root xmlns:xsi=\"{XSI}\" xsi:noNamespaceSchemaLocation=\"mem://nons.xsd\"><n>x</n></root>"
    );
    check(
        &xml,
        &["InvalidTextContent|Error|Some(1):Some(111)|element 'n': 'x' is not a valid xs:int"],
    );
}

#[test]
fn prefixed_root_and_custom_xsi_prefix() {
    let xml = format!(
        "<t:root xmlns:t=\"urn:t\" xmlns:inst=\"{XSI}\"\n  inst:schemaLocation=\"urn:t mem://t.xsd\">\n  <t:n>z</t:n>\n</t:root>"
    );
    check(
        &xml,
        &["InvalidTextContent|Error|Some(3):Some(8)|element 't:n': 'z' is not a valid xs:int"],
    );
}

#[test]
fn xsi_prefix_bound_elsewhere_is_not_a_hint() {
    check(
        "<root xmlns=\"urn:t\" xmlns:xsi=\"urn:not-xsi\" xsi:schemaLocation=\"urn:t mem://t.xsd\"><n>1</n></root>",
        &[NO_HINTS],
    );
}

#[test]
fn bom_declaration_comment_pi_doctype_before_root() {
    let xml = format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- leading comment -->\n<?app data?>\n<!DOCTYPE root [\n  <!ENTITY e \"abc\">\n]>\n{}",
        doc_t("\n  <n>&e;</n>\n  <n>1</n>\n")
    );
    check(
        &xml,
        &["InvalidTextContent|Error|Some(9):Some(6)|element 'n': 'abc' is not a valid xs:int"],
    );
}

#[test]
fn crlf_line_endings() {
    let xml = doc_t("\n  <n>1</n>\n  <n>abc</n>\n").replace('\n', "\r\n");
    check(
        &xml,
        &["InvalidTextContent|Error|Some(4):Some(6)|element 'n': 'abc' is not a valid xs:int"],
    );
}

#[test]
fn long_prolog_spanning_many_buffers() {
    let comment = "x".repeat(40_000);
    let xml = format!(
        "<?xml version=\"1.0\"?>\n<!--{comment}-->\n{}",
        doc_t("\n<n>1</n><n>q</n>")
    );
    check(
        &xml,
        &["InvalidTextContent|Error|Some(5):Some(12)|element 'n': 'q' is not a valid xs:int"],
    );
}

#[test]
fn malformed_prolog() {
    check(
        "<?xml version=\"1.0\"?>\n<!-- a -- b -->\n<root/>",
        &["Err(parse error: not well-formed: '--' is not allowed inside a comment)"],
    );
}

#[test]
fn unterminated_doctype() {
    check(
        "<!DOCTYPE root [\n",
        &[
            "Err(parse error: parse error at position 17: syntax error: DOCTYPE not closed: `>` not found before end of input)",
        ],
    );
}

#[test]
fn prolog_only() {
    check(
        "<?xml version=\"1.0\"?>\n<!-- nothing -->\n",
        &["Err(parse error: not well-formed: no document element found)"],
    );
}

#[test]
fn empty_input() {
    check(
        "",
        &["Err(parse error: not well-formed: no document element found)"],
    );
}

#[test]
fn max_errors_and_aggregation_apply() {
    let xml = doc_t("<n>a</n><n>a</n><n>a</n>");
    let capped = describe(
        Validator::from(xml.as_str())
            .max_errors(1)
            .run_with(MemFetcher::new()),
    );
    let aggregated = describe(
        Validator::from(xml.as_str())
            .aggregate_errors()
            .run_with(MemFetcher::new()),
    );
    let first = "InvalidTextContent|Error|Some(2):Some(49)|element 'n': 'a' is not a valid xs:int";
    assert_eq!(capped, [first]);
    assert_eq!(aggregated, [format!("{first} x3")]);
}
