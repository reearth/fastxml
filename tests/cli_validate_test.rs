//! End-to-end tests for the `fastxml-validate` CLI.
#![cfg(feature = "ureq")]

use std::path::Path;
use std::process::{Command, Output};

const SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns="urn:t" elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType><xs:sequence>
      <xs:element name="n" type="xs:int" maxOccurs="unbounded"/>
    </xs:sequence></xs:complexType>
  </xs:element>
</xs:schema>"#;

fn cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fastxml-validate"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `<dir>/schemas/s.xsd` plus `<dir>/data/sub/{rel,rel_ok}.xml` that point at
/// it with a relative `../../schemas/s.xsd`.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("schemas")).unwrap();
    std::fs::create_dir_all(dir.path().join("data/sub")).unwrap();
    std::fs::create_dir_all(dir.path().join("elsewhere")).unwrap();
    std::fs::write(dir.path().join("schemas/s.xsd"), SCHEMA).unwrap();
    let doc = |body: &str| {
        format!(
            r#"<root xmlns="urn:t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:t ../../schemas/s.xsd">{body}</root>"#
        )
    };
    std::fs::write(dir.path().join("data/sub/rel.xml"), doc("<n>abc</n>")).unwrap();
    std::fs::write(dir.path().join("data/sub/rel_ok.xml"), doc("<n>1</n>")).unwrap();
    std::fs::write(
        dir.path().join("data/nosl.xml"),
        r#"<root xmlns="urn:t"><n>abc</n></root>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("data/nosl_ok.xml"),
        r#"<root xmlns="urn:t"><n>1</n></root>"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("data/missing.xml"),
        r#"<root xmlns="urn:t" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:t nowhere/missing.xsd"><n>1</n></root>"#,
    )
    .unwrap();
    dir
}

#[test]
fn schema_option_is_used() {
    let f = fixture();
    let schema = f.path().join("schemas/s.xsd");
    let schema = schema.to_str().unwrap();
    let bad = f.path().join("data/nosl.xml");
    let ok = f.path().join("data/nosl_ok.xml");

    let out = cli(f.path(), &["--schema", schema, bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("not a valid xs:int"),
        "{}",
        stdout(&out)
    );

    let out = cli(f.path(), &["--schema", schema, ok.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
}

#[test]
fn unloadable_schema_option_is_a_usage_error() {
    let f = fixture();
    let ok = f.path().join("data/nosl_ok.xml");
    let out = cli(f.path(), &["--schema", "nope.xsd", ok.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nope.xsd"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn relative_schema_location_resolves_against_the_file_not_the_cwd() {
    let f = fixture();
    let elsewhere = f.path().join("elsewhere");

    let out = cli(&elsewhere, &["../data/sub/rel.xml"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("not a valid xs:int"),
        "{}",
        stdout(&out)
    );

    let out = cli(&elsewhere, &["../data/sub/rel_ok.xml"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
}

#[test]
fn schema_that_cannot_be_loaded_fails_the_file_visibly() {
    let f = fixture();
    let out = cli(f.path(), &["data/missing.xml"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    // Visible without -v.
    assert!(
        stdout(&out).contains("nowhere/missing.xsd"),
        "{}",
        stdout(&out)
    );

    let out = cli(f.path(), &["-j", "data/missing.xml"]);
    assert_eq!(out.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["files"][0]["valid"], false);
    assert_eq!(json["summary"]["invalid"], 1);
}

#[test]
fn document_without_schema_fails_unless_schema_given() {
    let f = fixture();
    let out = cli(f.path(), &["data/nosl_ok.xml"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).contains("no schema"), "{}", stdout(&out));
}

#[test]
fn help_documents_exit_codes() {
    let f = fixture();
    let out = cli(f.path(), &["--help"]);
    let help = stdout(&out);
    assert!(help.contains("Exit codes"), "{help}");
}
