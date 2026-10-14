//! Which attributes an XSD element may carry, and which of them depend on
//! whether the element is a top-level declaration.
//!
//! Attributes with a namespace (`xml:lang`, foreign attributes) are always
//! allowed; unqualified ones must be among the element's attributes in the
//! schema for schemas. Attributes XSD 1.1 adds are allowed too, so a 1.1
//! schema is not rejected for them.

use std::collections::HashMap;

use crate::error::Result;
use crate::schema::error::SchemaError;

use super::super::XsdParser;
use super::super::stack_frame::StackFrame;

/// Unqualified attributes each XSD element allows besides `id`. `None` for
/// elements not listed (they are not checked).
fn allowed(local: &str) -> Option<&'static [&'static str]> {
    Some(match local {
        "schema" => &[
            "attributeFormDefault",
            "blockDefault",
            "elementFormDefault",
            "finalDefault",
            "targetNamespace",
            "version",
            "defaultAttributes",
            "xpathDefaultNamespace",
        ],
        "element" => &[
            "abstract",
            "block",
            "default",
            "final",
            "fixed",
            "form",
            "maxOccurs",
            "minOccurs",
            "name",
            "nillable",
            "ref",
            "substitutionGroup",
            "type",
            "targetNamespace",
        ],
        "attribute" => &[
            "default",
            "fixed",
            "form",
            "name",
            "ref",
            "type",
            "use",
            "targetNamespace",
            "inheritable",
        ],
        "complexType" => &[
            "abstract",
            "block",
            "final",
            "mixed",
            "name",
            "defaultAttributesApply",
        ],
        "simpleType" => &["final", "name"],
        "group" => &["maxOccurs", "minOccurs", "name", "ref"],
        "attributeGroup" => &["name", "ref"],
        "sequence" | "choice" | "all" => &["maxOccurs", "minOccurs"],
        "any" => &[
            "maxOccurs",
            "minOccurs",
            "namespace",
            "processContents",
            "notNamespace",
            "notQName",
        ],
        "anyAttribute" => &["namespace", "processContents", "notNamespace", "notQName"],
        "complexContent" => &["mixed"],
        "simpleContent" | "annotation" => &[],
        "restriction" | "extension" => &["base"],
        "list" => &["itemType"],
        "union" => &["memberTypes"],
        "enumeration" | "pattern" => &["value"],
        "length" | "minLength" | "maxLength" | "minInclusive" | "maxInclusive" | "minExclusive"
        | "maxExclusive" | "totalDigits" | "fractionDigits" | "whiteSpace" | "explicitTimezone" => {
            &["value", "fixed"]
        }
        "include" | "redefine" | "override" => &["schemaLocation"],
        "import" => &["namespace", "schemaLocation"],
        "notation" => &["name", "public", "system"],
        "unique" | "key" => &["name", "ref"],
        "keyref" => &["name", "ref", "refer"],
        "selector" | "field" => &["xpath", "xpathDefaultNamespace"],
        "appinfo" | "documentation" => &["source"],
        _ => return None,
    })
}

fn invalid(message: String) -> Result<()> {
    Err(SchemaError::InvalidSchema { message }.into())
}

impl XsdParser {
    /// Rejects unqualified attributes `local` does not allow, and the
    /// attributes a top-level or a local declaration must not have.
    pub(in crate::schema::xsd::parser) fn check_allowed_attributes(
        &self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        let Some(allowed) = allowed(local) else {
            return Ok(());
        };
        let xsd_prefix = match &self.xsd_prefix {
            Some(Some(prefix)) => Some(prefix.as_str()),
            _ => None,
        };
        for name in attrs.keys() {
            if let Some((prefix, _)) = name.split_once(':') {
                // Attributes in the XSD namespace itself are not allowed.
                if Some(prefix) == xsd_prefix {
                    return invalid(format!("attribute '{name}' is not allowed on '{local}'"));
                }
                continue;
            }
            if name == "xmlns" || name == "id" {
                continue;
            }
            if !allowed.contains(&name.as_str()) {
                return invalid(format!("attribute '{name}' is not allowed on '{local}'"));
            }
        }

        // no-xmlns: an attribute declaration cannot be named xmlns.
        if local == "attribute" && attrs.get("name").is_some_and(|n| n.trim() == "xmlns") {
            return invalid("an attribute cannot be named 'xmlns'".to_string());
        }

        let top_level = matches!(
            self.stack.last(),
            Some(StackFrame::Schema | StackFrame::Redefine(_) | StackFrame::Override(_)) | None
        );
        if top_level && matches!(local, "complexType" | "simpleType") && !attrs.contains_key("name")
        {
            return invalid(format!("a top-level '{local}' requires a 'name'"));
        }
        // The model group of a named group definition has no occurrence
        // bounds of its own (the group references carry them).
        let in_group_definition = self
            .child_state_stack
            .last()
            .is_some_and(|p| p.name == "group");
        let forbidden: &[&str] = match (local, top_level) {
            ("element", true) => &["ref", "form", "minOccurs", "maxOccurs"],
            ("element", false) => &["abstract", "final", "substitutionGroup"],
            ("attribute", true) => &["ref", "form", "use"],
            ("group" | "attributeGroup", true) => &["ref", "minOccurs", "maxOccurs"],
            ("complexType" | "simpleType", false) => &["name"],
            ("sequence" | "choice" | "all", false) if in_group_definition => {
                &["minOccurs", "maxOccurs"]
            }
            _ => &[],
        };
        if let Some(name) = forbidden.iter().find(|a| attrs.contains_key(**a)) {
            let place = if top_level { "a top-level" } else { "a local" };
            return invalid(format!(
                "attribute '{name}' is not allowed on {place} '{local}'"
            ));
        }
        Ok(())
    }
}
