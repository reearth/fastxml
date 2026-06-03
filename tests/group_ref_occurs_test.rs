//! Occurrence-bound propagation for `<xs:group ref="...">`.
//!
//! When a complex type pulls in a named model group via `<xs:group ref>`, the
//! `minOccurs` / `maxOccurs` declared at the *reference site* must reach the
//! group's members. Earlier the parser read those bounds but dropped them, so a
//! `maxOccurs="unbounded"` group ref wrongly rejected every document with more
//! than one member.
//!
//! The group is repeated `[N, M]` times, so a member with its own bounds
//! `(j, k)` reached through a group ref of `(N, M)` has effective bounds
//! `(j*N, k*M)` — both min and max multiply. This is verified with one
//! table-driven test against both validators (DOM and OnePass).

use std::sync::Arc;

use fastxml::Parser;
use fastxml::ValidationErrorType;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::{Schema, Validator};

const NS: &str = "http://example.com/occurs";

/// Builds a single-namespace schema whose `ContainerType` reaches its `item`
/// members through `<xs:group ref="t:ItemGroup">`. The group ref's bounds
/// (`group_min`/`group_max`) and the member's own bounds (`elem_min`/`elem_max`)
/// are both parameterized so a test can exercise the full `(j*N, k*M)` product.
fn schema(group_min: &str, group_max: &str, elem_min: &str, elem_max: &str) -> Arc<CompiledSchema> {
    let xsd = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:t="{NS}"
           targetNamespace="{NS}"
           elementFormDefault="qualified">

  <xs:element name="Container" type="t:ContainerType"/>

  <xs:complexType name="ContainerType">
    <xs:sequence>
      <xs:group ref="t:ItemGroup" minOccurs="{group_min}" maxOccurs="{group_max}"/>
    </xs:sequence>
  </xs:complexType>

  <!-- `item` is a LOCAL element declared inside the named group, reached only
       via the <xs:group ref> above. -->
  <xs:group name="ItemGroup">
    <xs:sequence>
      <xs:element name="item" type="xs:string" minOccurs="{elem_min}" maxOccurs="{elem_max}"/>
    </xs:sequence>
  </xs:group>
</xs:schema>"#
    );
    let compiled = Schema::from_xsd(xsd.as_bytes()).expect("Failed to compile occurs schema");
    Arc::new(compiled)
}

/// Builds a `Container` instance holding `n` `item` children.
fn container_with_items(n: usize) -> String {
    let items: String = (0..n)
        .map(|i| format!("  <t:item>v{i}</t:item>\n"))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<t:Container xmlns:t=\"{NS}\">\n{items}</t:Container>"
    )
}

fn validate_dom(schema: &Arc<CompiledSchema>, xml: &str) -> Vec<fastxml::StructuredError> {
    let doc = Parser::from(xml).parse().expect("Failed to parse XML");
    Validator::from(&doc)
        .schema(schema.clone())
        .run()
        .expect("DOM validation failed")
        .into_entries()
}

fn validate_onepass(schema: &Arc<CompiledSchema>, xml: &str) -> Vec<fastxml::StructuredError> {
    Validator::from(xml)
        .schema(schema.clone())
        .run()
        .expect("OnePass validation failed")
        .into_entries()
}

fn is_valid(errors: &[fastxml::StructuredError]) -> bool {
    errors.iter().all(|e| !e.is_error())
}

fn has_error_type(errors: &[fastxml::StructuredError], ty: ValidationErrorType) -> bool {
    errors
        .iter()
        .filter(|e| e.is_error())
        .any(|e| e.error_type == ty)
}

/// Expected outcome for a single (schema, instance) row.
#[derive(Clone, Copy, Debug)]
enum Expect {
    Valid,
    TooFew,
    TooMany,
}

/// One row of the occurs table: a parameterized schema, the instance size, and
/// the expected validation outcome. Named fields make the failure dump readable.
#[derive(Debug)]
struct Case {
    group_min: &'static str,
    group_max: &'static str,
    elem_min: &'static str,
    elem_max: &'static str,
    n: usize,
    expect: Expect,
}

impl Case {
    /// Builds a case from grouped `(min, max)` bounds for the group ref and the
    /// member, so table rows stay compact: `Case::new(group, elem, n, expect)`.
    fn new(
        group: (&'static str, &'static str),
        elem: (&'static str, &'static str),
        n: usize,
        expect: Expect,
    ) -> Self {
        Self {
            group_min: group.0,
            group_max: group.1,
            elem_min: elem.0,
            elem_max: elem.1,
            n,
            expect,
        }
    }
}

/// Validates `case` with both validators and asserts the expected outcome. On
/// failure the message identifies the row (`case #i`) and dumps the whole `Case`.
fn check(index: usize, case: &Case) {
    let schema = schema(case.group_min, case.group_max, case.elem_min, case.elem_max);
    let xml = container_with_items(case.n);

    for (who, errors) in [
        ("DOM", validate_dom(&schema, &xml)),
        ("OnePass", validate_onepass(&schema, &xml)),
    ] {
        let ok = match case.expect {
            Expect::Valid => is_valid(&errors),
            Expect::TooFew => has_error_type(&errors, ValidationErrorType::TooFewOccurrences),
            Expect::TooMany => has_error_type(&errors, ValidationErrorType::TooManyOccurrences),
        };
        assert!(
            ok,
            "case #{index} [{who}] failed: {case:?}\n  errors: {errors:?}"
        );
    }
}

#[test]
fn group_ref_occurs_multiply_with_member_bounds() {
    use Expect::*;

    // Central schema: group(min=2, max=3) x member(min=2, max=4). The member's
    // effective bounds are the products (min, max) = (2*2, 4*3) = (4, 12). Since
    // 4 and 12 come from no single factor (2, 3, or 4), the boundary rows prove
    // both bounds are multiplied. `n` is the item count in the document, compared
    // against those effective bounds. The leading rows cover the min=0 relaxation,
    // an optional member (0*N = 0), and unbounded max.
    #[rustfmt::skip]
    let cases = [
        //        group (min, max)    elem (min, max)     n   expected
        Case::new(("0", "unbounded"), ("1", "1"),         0,  Valid),   // min 0 -> member optional, empty ok
        Case::new(("0", "unbounded"), ("1", "1"),         5,  Valid),   // max unbounded -> any count ok
        Case::new(("2", "unbounded"), ("0", "unbounded"), 0,  Valid),   // member optional (min 0*2=0), empty ok
        Case::new(("2", "3"),         ("2", "4"),         3,  TooFew),  // count 3 < effective min 4
        Case::new(("2", "3"),         ("2", "4"),         4,  Valid),   // count 4 == effective min 4
        Case::new(("2", "3"),         ("2", "4"),         12, Valid),   // count 12 == effective max 12
        Case::new(("2", "3"),         ("2", "4"),         13, TooMany), // count 13 > effective max 12
    ];

    for (i, case) in cases.iter().enumerate() {
        check(i, case);
    }
}
