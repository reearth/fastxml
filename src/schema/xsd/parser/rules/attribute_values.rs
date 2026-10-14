//! Lexical and enumeration rules on the attribute values of XSD elements.
//!
//! [`XsdParser::check_attribute_value_rules`] dispatches on the element kind;
//! each `check_*_attributes` function holds the checks for one kind, in the
//! order they are reported.

use std::collections::HashMap;

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::xsd::primitive::PrimitiveKind;

use super::super::XsdParser;

/// Values of an `xs:boolean` attribute.
const BOOL: &[&str] = &["true", "false", "1", "0"];

/// Values of a `form` / `*FormDefault` attribute.
const FORM: &[&str] = &["qualified", "unqualified"];

impl XsdParser {
    /// Enforces lexical and enumeration rules on the attributes of an XSD
    /// element: NCName-valued `name`, QName-valued references, enumerated
    /// values (`form`, `use`, `processContents`, …), boolean attributes,
    /// derivation-control sets, wildcard namespace lists, and the
    /// `ref`-excludes-other-attributes rules.
    pub(in crate::schema::xsd::parser) fn check_attribute_value_rules(
        &self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        check_name_is_ncname(local, attrs)?;

        match local {
            "element" => check_element_attributes(attrs),
            "attribute" => check_attribute_attributes(attrs),
            "group" | "attributeGroup" => check_qname(local, attrs, "ref"),
            "restriction" | "extension" => check_qname(local, attrs, "base"),
            "list" => check_qname(local, attrs, "itemType"),
            "union" => check_union_member_types(attrs),
            "keyref" => check_qname(local, attrs, "refer"),
            "complexType" => check_complex_type_attributes(attrs),
            "complexContent" => check_enum(local, attrs, "mixed", BOOL),
            "simpleType" => {
                check_derivation_set(local, attrs, "final", &["list", "union", "restriction"])
            }
            "schema" => check_schema_attributes(attrs),
            "any" | "anyAttribute" => check_wildcard_attributes(local, attrs),
            _ => Ok(()),
        }
    }
}

fn invalid(message: String) -> Result<()> {
    Err(SchemaError::InvalidSchema { message }.into())
}

/// A lexically valid QName: NCName, optionally prefixed.
fn is_valid_qname(value: &str) -> bool {
    match value.split_once(':') {
        Some((p, l)) => {
            PrimitiveKind::Ncname.validate(p).is_ok() && PrimitiveKind::Ncname.validate(l).is_ok()
        }
        None => PrimitiveKind::Ncname.validate(value).is_ok(),
    }
}

/// A QName-valued attribute, if present.
fn check_qname(local: &str, attrs: &HashMap<String, String>, attr: &str) -> Result<()> {
    let Some(value) = attrs.get(attr) else {
        return Ok(());
    };
    // QName attribute values are whitespace-collapsed.
    let value = value.trim();
    if !is_valid_qname(value) {
        return invalid(format!("'{value}' is not a valid QName in {local}/@{attr}"));
    }
    Ok(())
}

/// An enumerated attribute value, if present.
fn check_enum(
    local: &str,
    attrs: &HashMap<String, String>,
    attr: &str,
    allowed: &[&str],
) -> Result<()> {
    if let Some(value) = attrs.get(attr)
        && !allowed.contains(&value.as_str())
    {
        return invalid(format!("invalid value '{value}' for {local}/@{attr}"));
    }
    Ok(())
}

/// A derivation-control set: '#all' or a subset of `allowed`.
fn check_derivation_set(
    local: &str,
    attrs: &HashMap<String, String>,
    attr: &str,
    allowed: &[&str],
) -> Result<()> {
    if let Some(value) = attrs.get(attr) {
        let value = value.trim();
        if value != "#all"
            && let Some(bad) = value.split_whitespace().find(|t| !allowed.contains(t))
        {
            return invalid(format!("invalid token '{bad}' in {local}/@{attr}"));
        }
    }
    Ok(())
}

/// Rejects each of `conflicting` present alongside `ref`.
fn check_ref_excludes(
    attrs: &HashMap<String, String>,
    what: &str,
    conflicting: &[&str],
) -> Result<()> {
    if attrs.contains_key("ref") {
        for conflicting in conflicting {
            if attrs.contains_key(*conflicting) {
                return invalid(format!("{what} reference cannot also have '{conflicting}'"));
            }
        }
    }
    Ok(())
}

/// NCName-valued `name` attribute on the named component kinds.
fn check_name_is_ncname(local: &str, attrs: &HashMap<String, String>) -> Result<()> {
    if matches!(
        local,
        "element"
            | "attribute"
            | "complexType"
            | "simpleType"
            | "group"
            | "attributeGroup"
            | "notation"
    ) && let Some(name) = attrs.get("name")
        && PrimitiveKind::Ncname.validate(name).is_err()
    {
        return invalid(format!("'{name}' is not a valid NCName in {local}/@name"));
    }
    Ok(())
}

fn check_element_attributes(attrs: &HashMap<String, String>) -> Result<()> {
    const LOCAL: &str = "element";
    check_qname(LOCAL, attrs, "ref")?;
    check_qname(LOCAL, attrs, "type")?;
    check_qname(LOCAL, attrs, "substitutionGroup")?;
    check_enum(LOCAL, attrs, "form", FORM)?;
    check_enum(LOCAL, attrs, "abstract", BOOL)?;
    check_enum(LOCAL, attrs, "nillable", BOOL)?;
    check_derivation_set(
        LOCAL,
        attrs,
        "block",
        &["extension", "restriction", "substitution"],
    )?;
    check_derivation_set(LOCAL, attrs, "final", &["extension", "restriction"])?;
    check_ref_excludes(
        attrs,
        "element",
        &[
            "name",
            "type",
            "form",
            "block",
            "final",
            "default",
            "fixed",
            "nillable",
            "substitutionGroup",
            "abstract",
        ],
    )
}

fn check_attribute_attributes(attrs: &HashMap<String, String>) -> Result<()> {
    const LOCAL: &str = "attribute";
    check_qname(LOCAL, attrs, "ref")?;
    check_qname(LOCAL, attrs, "type")?;
    check_enum(LOCAL, attrs, "form", FORM)?;
    check_enum(LOCAL, attrs, "use", &["optional", "prohibited", "required"])?;
    // Note: use='prohibited' with fixed= is legal; only default=
    // requires use='optional' (enforced in content-model rules).
    check_ref_excludes(attrs, "attribute", &["name", "type", "form"])
}

/// Each whitespace-separated member of `union/@memberTypes` is a QName.
fn check_union_member_types(attrs: &HashMap<String, String>) -> Result<()> {
    if let Some(members) = attrs.get("memberTypes") {
        for member in members.split_whitespace() {
            if !is_valid_qname(member) {
                return invalid(format!(
                    "'{member}' is not a valid QName in union/@memberTypes"
                ));
            }
        }
    }
    Ok(())
}

fn check_complex_type_attributes(attrs: &HashMap<String, String>) -> Result<()> {
    const LOCAL: &str = "complexType";
    check_enum(LOCAL, attrs, "abstract", BOOL)?;
    check_enum(LOCAL, attrs, "mixed", BOOL)?;
    check_derivation_set(LOCAL, attrs, "block", &["extension", "restriction"])?;
    check_derivation_set(LOCAL, attrs, "final", &["extension", "restriction"])
}

fn check_schema_attributes(attrs: &HashMap<String, String>) -> Result<()> {
    const LOCAL: &str = "schema";
    // targetNamespace is an anyURI that must not be empty (no namespace is
    // written by leaving the attribute out).
    if attrs.get("targetNamespace").is_some_and(|t| t.is_empty()) {
        return invalid("targetNamespace cannot be empty".to_string());
    }
    check_enum(LOCAL, attrs, "elementFormDefault", FORM)?;
    check_enum(LOCAL, attrs, "attributeFormDefault", FORM)?;
    check_derivation_set(
        LOCAL,
        attrs,
        "blockDefault",
        &["extension", "restriction", "substitution"],
    )?;
    check_derivation_set(
        LOCAL,
        attrs,
        "finalDefault",
        &["extension", "restriction", "list", "union"],
    )
}

/// `processContents` and the `namespace` list of `any` / `anyAttribute`.
fn check_wildcard_attributes(local: &str, attrs: &HashMap<String, String>) -> Result<()> {
    check_enum(local, attrs, "processContents", &["strict", "lax", "skip"])?;
    if let Some(namespace) = attrs.get("namespace") {
        let value = namespace.trim();
        if value != "##any" && value != "##other" {
            for token in value.split_whitespace() {
                if token.starts_with("##") && token != "##targetNamespace" && token != "##local" {
                    return invalid(format!("invalid wildcard namespace token '{token}'"));
                }
            }
        }
    }
    Ok(())
}
