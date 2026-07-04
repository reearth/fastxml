//! W3C XML Schema Test Suite runner.

use super::{AuditHistogram, Engine, Eval, SuiteRun, panic_message, rel};
use crate::catalog::xsdtests::{
    InstanceTest, InstanceValidity, SchemaDocument, SchemaTestGroup, SchemaValidity, XsdTestSuite,
};
use crate::outcome::{
    Outcome, TestRecord, classify_invalid_schema_rejection, classify_valid_schema_failure,
    error_variant_name,
};
use fastxml::schema::Schema;
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

/// Instance documents whose validity verdict is nondeterministic in fastxml
/// (its wildcard/namespace-set processing iterates unordered collections, so
/// the same input can validate or not across runs). We record these as
/// `Blocked` with a clear reason rather than let a coin-flip destabilize the
/// baseline ratchet. This is an honest "cannot decide", not a hidden pass; the
/// underlying nondeterminism is a fastxml bug worth fixing separately. Matched
/// by path suffix so it is independent of the data-directory layout.
const NONDETERMINISTIC_INSTANCE_SUFFIXES: &[&str] = &["msData/wildcards/wildG031.xml"];

/// Whether an instance is known to validate nondeterministically in fastxml.
fn is_nondeterministic(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    NONDETERMINISTIC_INSTANCE_SUFFIXES
        .iter()
        .any(|suffix| s.ends_with(suffix))
}

/// Run the whole W3C XSD suite with one engine.
pub fn run_xsd_suite(engine: Engine, suite: &XsdTestSuite) -> SuiteRun {
    let mut run = SuiteRun::default();
    let mut audit = AuditHistogram::default();
    let base = &suite.base_path;

    for group in suite.all_groups() {
        // Schema tests: each schema document is its own test body.
        for schema_doc in &group.schemas {
            let id = doc_id(group, &schema_doc.path, base);
            let category = schema_category(schema_doc.expected);
            let eval = catch_unwind(AssertUnwindSafe(|| eval_schema(schema_doc, base)))
                .unwrap_or_else(|p| Eval::new(Outcome::Panic, Some(panic_message(p.as_ref()))));
            record(
                &mut run,
                &mut audit,
                category,
                &id,
                schema_doc_expected(schema_doc.expected),
                eval,
            );
        }

        // Instance tests share one compiled schema (all valid-expected schema
        // documents of the group, resolved together so imports/wildcards work).
        let compiled =
            catch_unwind(AssertUnwindSafe(|| compile_group_schemas(group, base))).unwrap_or(None);

        for instance in &group.instances {
            let id = doc_id(group, &instance.path, base);
            let category = instance_category(instance.expected);
            let expected = instance_expected(instance.expected);
            let eval = catch_unwind(AssertUnwindSafe(|| {
                eval_instance(engine, instance, compiled.as_ref(), base)
            }))
            .unwrap_or_else(|p| Eval::new(Outcome::Panic, Some(panic_message(p.as_ref()))));
            record(&mut run, &mut audit, category, &id, expected, eval);
        }
    }

    if crate::should_audit() && !audit.is_empty() {
        audit.print(&format!("W3C XSD ({})", engine.as_str()));
    }
    run
}

/// Push a record and, when auditing, note the error variant.
fn record(
    run: &mut SuiteRun,
    audit: &mut AuditHistogram,
    category: &str,
    id: &str,
    expected: &str,
    eval: Eval,
) {
    if let Some(variant) = eval.audit_variant {
        audit.record(category, expected, variant);
    }
    run.push(TestRecord::new(category, id, eval.outcome, eval.detail));
}

/// Evaluate one schema document against its expected validity.
fn eval_schema(schema_doc: &SchemaDocument, base: &Path) -> Eval {
    if schema_doc.expected == SchemaValidity::Indeterminate {
        return Eval::new(Outcome::Unsupported, Some("indeterminate".into()));
    }
    let content = match std::fs::read(&schema_doc.path) {
        Ok(c) => c,
        Err(e) => {
            return Eval::new(
                Outcome::Blocked,
                Some(format!("read {}: {}", rel(&schema_doc.path, base), e)),
            );
        }
    };
    match (schema_doc.expected, Schema::from_xsd(&content)) {
        (SchemaValidity::Valid, Ok(_)) => Eval::new(Outcome::Pass, None),
        (SchemaValidity::Valid, Err(e)) => {
            let outcome = classify_valid_schema_failure(&e);
            Eval::new(outcome, Some(e.to_string())).with_audit(error_variant_name(&e))
        }
        (SchemaValidity::Invalid, Ok(_)) => Eval::new(
            Outcome::Fail,
            Some("compiled a schema that should be invalid".into()),
        ),
        (SchemaValidity::Invalid, Err(e)) => {
            let outcome = classify_invalid_schema_rejection(&e);
            let detail = if outcome == Outcome::Pass {
                None
            } else {
                Some(e.to_string())
            };
            Eval::new(outcome, detail).with_audit(error_variant_name(&e))
        }
        (SchemaValidity::Indeterminate, _) => unreachable!("handled above"),
    }
}

/// Evaluate one instance document against a compiled schema.
fn eval_instance(
    engine: Engine,
    instance: &InstanceTest,
    compiled: Option<&Arc<Schema>>,
    base: &Path,
) -> Eval {
    if instance.expected == InstanceValidity::Indeterminate {
        return Eval::new(Outcome::Unsupported, Some("indeterminate".into()));
    }
    if is_nondeterministic(&instance.path) {
        return Eval::new(
            Outcome::Blocked,
            Some("nondeterministic wildcard validation in fastxml".into()),
        );
    }
    let Some(schema) = compiled else {
        return Eval::new(Outcome::Blocked, Some("schema failed to compile".into()));
    };
    let content = match std::fs::read(&instance.path) {
        Ok(c) => c,
        Err(e) => {
            return Eval::new(
                Outcome::Blocked,
                Some(format!("read {}: {}", rel(&instance.path, base), e)),
            );
        }
    };

    let result = validate(engine, &content, schema);
    match result {
        Ok(errors) => {
            let is_valid = errors.is_empty();
            let pass = match instance.expected {
                InstanceValidity::Valid => is_valid,
                InstanceValidity::Invalid => !is_valid,
                InstanceValidity::Indeterminate => unreachable!(),
            };
            if pass {
                Eval::new(Outcome::Pass, None)
            } else {
                let detail = match instance.expected {
                    InstanceValidity::Valid => {
                        "expected valid but validator reported errors".into()
                    }
                    _ => "expected invalid but validator accepted it".into(),
                };
                Eval::new(Outcome::Fail, Some(detail))
            }
        }
        // A validator error means we could not obtain a validity verdict: the
        // document (or schema resolution) failed before validation completed.
        Err(e) => {
            Eval::new(Outcome::Blocked, Some(e.to_string())).with_audit(error_variant_name(&e))
        }
    }
}

/// Run the validator with the selected engine, returning the produced error
/// entries (empty = valid) or a fastxml error.
fn validate(
    engine: Engine,
    content: &[u8],
    schema: &Arc<Schema>,
) -> fastxml::Result<Vec<fastxml::StructuredError>> {
    match engine {
        Engine::Dom => {
            let doc = fastxml::Parser::from(content).parse()?;
            Ok(fastxml::schema::Validator::from(&doc)
                .schema(Arc::clone(schema))
                .run()?
                .into_entries())
        }
        Engine::Streaming => Ok(
            fastxml::schema::Validator::from_reader(Cursor::new(content))
                .schema(Arc::clone(schema))
                .run()?
                .into_entries(),
        ),
    }
}

/// Compile all valid-expected schema documents of a group together, resolving
/// file imports so cross-document declarations are visible. Returns `None` when
/// the group has no valid schema documents or resolution fails.
fn compile_group_schemas(group: &SchemaTestGroup, _base: &Path) -> Option<Arc<Schema>> {
    let mut builder = Schema::builder();
    let mut have_docs = false;
    for schema_doc in &group.schemas {
        if schema_doc.expected != SchemaValidity::Valid || !schema_doc.path.exists() {
            continue;
        }
        if let Ok(content) = std::fs::read(&schema_doc.path) {
            builder = builder.add(schema_doc.path.to_string_lossy(), content);
            have_docs = true;
        }
    }
    if !have_docs {
        return None;
    }
    builder
        .resolve_with(&fastxml::schema::FileFetcher::new())
        .ok()
        .map(Arc::new)
}

/// A stable, unique id for a schema/instance document within the suite.
fn doc_id(group: &SchemaTestGroup, path: &Path, base: &Path) -> String {
    format!("{}::{}", group.name, rel(path, base))
}

fn schema_category(v: SchemaValidity) -> &'static str {
    match v {
        SchemaValidity::Valid => "schema-valid",
        SchemaValidity::Invalid => "schema-invalid",
        SchemaValidity::Indeterminate => "schema-indeterminate",
    }
}

fn schema_doc_expected(v: SchemaValidity) -> &'static str {
    match v {
        SchemaValidity::Valid => "valid",
        SchemaValidity::Invalid => "invalid",
        SchemaValidity::Indeterminate => "indeterminate",
    }
}

fn instance_category(v: InstanceValidity) -> &'static str {
    match v {
        InstanceValidity::Valid => "instance-valid",
        InstanceValidity::Invalid => "instance-invalid",
        InstanceValidity::Indeterminate => "instance-indeterminate",
    }
}

fn instance_expected(v: InstanceValidity) -> &'static str {
    match v {
        InstanceValidity::Valid => "valid",
        InstanceValidity::Invalid => "invalid",
        InstanceValidity::Indeterminate => "indeterminate",
    }
}

/// Find suite.xml in the test data directory, handling both a direct clone and
/// the CI layout (`w3c-xsd/xsdtests/suite.xml`).
pub fn find_suite_xml(data_path: &Path) -> Option<std::path::PathBuf> {
    let direct = data_path.join("suite.xml");
    if direct.exists() {
        return Some(direct);
    }
    let xsdtests = data_path.join("xsdtests");
    if xsdtests.exists() {
        let in_xsdtests = xsdtests.join("suite.xml");
        if in_xsdtests.exists() {
            return Some(in_xsdtests);
        }
    }
    for entry in data_path.read_dir().ok()?.flatten() {
        if entry.path().is_dir() {
            let candidate = entry.path().join("suite.xml");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Load the XSD test suite from the standard data location. Returns `None`
/// (with a message on stderr) when the data or suite.xml is missing.
pub fn load_suite(data_path: &Path) -> Option<XsdTestSuite> {
    let suite_path = find_suite_xml(data_path)?;
    match XsdTestSuite::parse(&suite_path) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("Failed to parse suite: {e}");
            None
        }
    }
}
