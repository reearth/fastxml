//! `resolve_schema_from_xml` / `resolve_schema_from_file` /
//! `export_schemas_from_xml`: directory safety, base directory, error
//! propagation and the export rewrite.

use std::path::Path;

use fastxml::schema::export::export_schemas_from_xml;
use fastxml::schema::{
    DefaultFetcher, ResolveOptions, resolve_schema_from_file, resolve_schema_from_xml,
};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

const SCHEMA: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:x" xmlns="urn:x" elementFormDefault="qualified">
  <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

fn write(path: &Path, content: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn doc_for(location: &str) -> String {
    format!(
        r#"<root xmlns="urn:x" xmlns:xsi="{XSI}" xsi:schemaLocation="urn:x {location}">v</root>"#
    )
}

#[test]
fn caller_export_dir_with_unrelated_files_is_never_wiped() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("s.xsd");
    write(&schema, SCHEMA);
    let export = tmp.path().join("my_project");
    write(&export.join("IMPORTANT.txt"), "keep me");

    let xml = doc_for(&schema.display().to_string());
    let result = resolve_schema_from_xml(
        xml.as_bytes(),
        &DefaultFetcher::new(),
        &ResolveOptions::default().export_dir(&export),
    );
    assert!(result.is_err(), "a non-empty export_dir must be refused");
    assert_eq!(
        std::fs::read_to_string(export.join("IMPORTANT.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn caller_export_dir_that_is_empty_or_new_receives_the_schemas() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("s.xsd");
    write(&schema, SCHEMA);
    let export = tmp.path().join("out/new");

    let xml = doc_for(&schema.display().to_string());
    let resolved = resolve_schema_from_xml(
        xml.as_bytes(),
        &DefaultFetcher::new(),
        &ResolveOptions::default().export_dir(&export),
    )
    .unwrap();
    assert!(!resolved.is_builtin());
    assert_eq!(resolved.export_dir.as_deref(), Some(export.as_path()));
    assert!(resolved.entry_schema_path().unwrap().exists());
}

#[test]
fn default_export_dir_is_unique_and_removed_unless_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let schema = tmp.path().join("s.xsd");
    write(&schema, SCHEMA);
    let xml = doc_for(&schema.display().to_string());
    let fetcher = DefaultFetcher::new();

    let kept = ResolveOptions::default().keep_export_dir();
    let a = resolve_schema_from_xml(xml.as_bytes(), &fetcher, &kept).unwrap();
    let b = resolve_schema_from_xml(xml.as_bytes(), &fetcher, &kept).unwrap();
    let (da, db) = (a.export_dir.clone().unwrap(), b.export_dir.clone().unwrap());
    assert_ne!(da, db, "each call needs its own directory");
    assert!(a.entry_schema_path().unwrap().exists());
    assert!(b.entry_schema_path().unwrap().exists());
    std::fs::remove_dir_all(da).unwrap();
    std::fs::remove_dir_all(db).unwrap();

    let not_kept =
        resolve_schema_from_xml(xml.as_bytes(), &fetcher, &ResolveOptions::default()).unwrap();
    assert!(!not_kept.is_builtin());
    assert!(not_kept.export_dir.is_none());
    assert!(!not_kept.compiled.elements_ns.is_empty());
}

#[test]
fn base_dir_resolves_relative_schema_locations() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("xmldir/schemas/s.xsd"), SCHEMA);
    let xml = doc_for("schemas/s.xsd");
    write(&tmp.path().join("xmldir/doc.xml"), &xml);

    let resolved = resolve_schema_from_xml(
        xml.as_bytes(),
        &DefaultFetcher::new(),
        &ResolveOptions::with_base_dir(tmp.path().join("xmldir")),
    )
    .unwrap();
    assert!(!resolved.is_builtin());
    assert!(!resolved.compiled.elements_ns.is_empty());

    let resolved =
        resolve_schema_from_file(tmp.path().join("xmldir/doc.xml"), &DefaultFetcher::new())
            .unwrap();
    assert!(!resolved.is_builtin());
    assert!(!resolved.compiled.elements_ns.is_empty());
}

#[test]
fn unloadable_schemas_are_errors_not_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    let fetcher = DefaultFetcher::new();
    let opts = ResolveOptions::default();

    // Unparseable schema.
    let broken = tmp.path().join("broken.xsd");
    write(
        &broken,
        "<xs:schema xmlns:xs='http://www.w3.org/2001/XMLSchema'><xs:element",
    );
    let r = resolve_schema_from_xml(
        doc_for(&broken.display().to_string()).as_bytes(),
        &fetcher,
        &opts,
    );
    assert!(r.is_err(), "broken schema must be an error: {r:?}");

    // Missing include.
    let main = tmp.path().join("main.xsd");
    write(
        &main,
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:x"><xs:include schemaLocation="missing.xsd"/></xs:schema>"#,
    );
    let r = resolve_schema_from_xml(
        doc_for(&main.display().to_string()).as_bytes(),
        &fetcher,
        &opts,
    );
    let err = r.expect_err("missing include must be an error").to_string();
    assert!(err.contains("missing.xsd"), "{err}");

    // Unreachable entry.
    let r = resolve_schema_from_xml(
        doc_for(&tmp.path().join("nope.xsd").display().to_string()).as_bytes(),
        &fetcher,
        &opts,
    );
    assert!(r.is_err());

    // No hints at all is reported truthfully as builtin.
    let r = resolve_schema_from_xml(b"<root/>", &fetcher, &opts).unwrap();
    assert!(r.is_builtin());
}

#[test]
fn non_utf8_schema_is_exported_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let mut latin1: Vec<u8> = br#"<?xml version="1.0" encoding="ISO-8859-1"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:x">
  <xs:annotation><xs:documentation>caf"#
        .to_vec();
    latin1.push(0xE9); // 'é' in ISO-8859-1
    latin1.extend_from_slice(
        br#"</xs:documentation></xs:annotation>
  <xs:element name="root" type="xs:string"/>
</xs:schema>"#,
    );
    let schema = tmp.path().join("latin1.xsd");
    write(&schema, &latin1);
    let out = tmp.path().join("out");

    let xml = doc_for(&schema.display().to_string());
    let result = export_schemas_from_xml(xml.as_bytes(), &out, &DefaultFetcher::new()).unwrap();
    let file = result.entry_filename.unwrap();
    assert_eq!(std::fs::read(out.join(file)).unwrap(), latin1);

    let resolved = resolve_schema_from_xml(
        xml.as_bytes(),
        &DefaultFetcher::new(),
        &ResolveOptions::default(),
    )
    .unwrap();
    assert!(!resolved.is_builtin());
    assert!(!resolved.compiled.elements_ns.is_empty());
}

fn exported_include_target(out: &Path, main_file: &str) -> String {
    let main = std::fs::read_to_string(out.join(main_file)).unwrap();
    let start = main.find("<xs:include schemaLocation=\"").unwrap() + 28;
    let end = start + main[start..].find('"').unwrap();
    std::fs::read_to_string(out.join(&main[start..end])).unwrap()
}

#[test]
fn bare_relative_include_is_rewritten_to_its_own_target() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("A/main.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a">
  <xs:import namespace="urn:b" schemaLocation="../B/types.xsd"/>
  <xs:include schemaLocation="types.xsd"/>
</xs:schema>"#,
    );
    write(
        &tmp.path().join("A/types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a"><!-- A types --></xs:schema>"#,
    );
    write(
        &tmp.path().join("B/types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:b"><!-- B types --></xs:schema>"#,
    );
    let main = tmp.path().join("A/main.xsd");
    let xml = format!(
        r#"<root xmlns="urn:a" xmlns:xsi="{XSI}" xsi:schemaLocation="urn:a {}"/>"#,
        main.display()
    );

    for run in 0..10 {
        let out = tmp.path().join(format!("out{run}"));
        let result = export_schemas_from_xml(xml.as_bytes(), &out, &DefaultFetcher::new()).unwrap();
        assert_eq!(result.schema_count, 3);
        let target = exported_include_target(&out, result.entry_filename.as_deref().unwrap());
        assert!(
            target.contains("A types"),
            "run {run}: include points at {target}"
        );
    }
}

#[test]
fn filenames_differing_only_in_case_do_not_collide() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("A/main.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a">
  <xs:include schemaLocation="types.xsd"/>
  <xs:import namespace="urn:c" schemaLocation="../C/Types.xsd"/>
</xs:schema>"#,
    );
    write(
        &tmp.path().join("A/types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:a"/>"#,
    );
    write(
        &tmp.path().join("C/Types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:c"/>"#,
    );
    let main = tmp.path().join("A/main.xsd");
    let xml = format!(
        r#"<root xmlns="urn:a" xmlns:xsi="{XSI}" xsi:schemaLocation="urn:a {}"/>"#,
        main.display()
    );
    let out = tmp.path().join("out");
    let result = export_schemas_from_xml(xml.as_bytes(), &out, &DefaultFetcher::new()).unwrap();
    assert_eq!(result.schema_count, 3);

    let mut names: Vec<String> = result
        .uri_to_filename
        .values()
        .map(|f| f.to_lowercase())
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 3, "{:?}", result.uri_to_filename);
    // All three schemas plus catalog.xml are on disk.
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 4);
}
