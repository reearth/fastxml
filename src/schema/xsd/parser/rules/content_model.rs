//! Per-element content rules from the schema for schemas.
//!
//! [`XsdParser::check_content_model_rules`] runs the checks on the element
//! itself, then the checks against its parent's bookkeeping, then the
//! parent-kind-specific child rules (`check_*_child` functions), in the
//! order they are reported.

use std::collections::HashMap;

use crate::error::Result;
use crate::schema::error::SchemaError;
use crate::schema::xsd::types::FormDefault;

use super::super::{ChildState, XsdParser};

impl XsdParser {
    /// Enforces per-element content rules from the schema for schemas:
    /// which children an XSD element admits, how many, and which attributes
    /// are required.
    pub(in crate::schema::xsd::parser) fn check_content_model_rules(
        &mut self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        check_required_attributes(local, attrs)?;

        // xs:schema is only allowed as the document root.
        if local == "schema" && !self.child_state_stack.is_empty() {
            return invalid("'schema' is only allowed as the document root".to_string());
        }

        self.check_group_definition_placement(local, attrs)?;
        self.check_duplicate_top_level_name(local, attrs)?;

        let attribute_form_qualified = self.schema.attribute_form_default == FormDefault::Qualified;
        let Some(parent) = self.child_state_stack.last_mut() else {
            return Ok(());
        };

        check_reference_has_no_children(parent, local)?;
        check_duplicate_attribute_use(parent, local, attrs, attribute_form_qualified)?;
        check_any_attribute_count(parent, local)?;
        check_allowed_child(parent, local, attrs)
    }

    /// Named group / attributeGroup definitions are top-level only; uses
    /// elsewhere must be references (src-attribute_group / src-model_group).
    fn check_group_definition_placement(
        &self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        if !matches!(local, "group" | "attributeGroup") || self.child_state_stack.is_empty() {
            return Ok(());
        }
        let parent_is_top = self
            .child_state_stack
            .last()
            .is_some_and(|p| matches!(p.name.as_str(), "schema" | "redefine" | "override"));
        if parent_is_top {
            if attrs.contains_key("ref") {
                return invalid(format!("top-level {local} cannot have 'ref'"));
            }
            if !attrs.contains_key("name") {
                return invalid(format!("top-level {local} requires 'name'"));
            }
        } else {
            if attrs.contains_key("name") {
                return invalid(format!("nested {local} cannot have 'name'"));
            }
            if !attrs.contains_key("ref") {
                return invalid(format!("nested {local} requires 'ref'"));
            }
        }
        Ok(())
    }

    /// Duplicate top-level component names within one document (simple
    /// and complex types share a symbol space).
    fn check_duplicate_top_level_name(
        &mut self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        if self
            .child_state_stack
            .last()
            .is_some_and(|p| p.name == "schema")
            && let Some(name) = attrs.get("name")
        {
            let space = match local {
                "complexType" | "simpleType" => Some("type"),
                "element" => Some("element"),
                "attribute" => Some("attribute"),
                "group" => Some("group"),
                "attributeGroup" => Some("attributeGroup"),
                "notation" => Some("notation"),
                _ => None,
            };
            if let Some(space) = space
                && !self.seen_top_level.insert(format!("{space}:{name}"))
            {
                return invalid(format!("duplicate top-level {local} '{name}'"));
            }
        }
        Ok(())
    }
}

fn invalid(message: String) -> Result<()> {
    Err(SchemaError::InvalidSchema { message }.into())
}

fn not_allowed_inside(local: &str, parent: &str) -> Result<()> {
    invalid(format!(
        "element '{}' is not allowed inside {}",
        local, parent
    ))
}

fn is_particle(local: &str) -> bool {
    matches!(local, "group" | "all" | "choice" | "sequence")
}

/// Required attributes on the element itself.
fn check_required_attributes(local: &str, attrs: &HashMap<String, String>) -> Result<()> {
    match local {
        "notation" => {
            // Per the XSD 1.0 errata, at least one of public/system.
            if !attrs.contains_key("name")
                || (!attrs.contains_key("public") && !attrs.contains_key("system"))
            {
                return invalid(
                    "notation requires 'name' and at least one of 'public'/'system'".to_string(),
                );
            }
        }
        "selector" | "field" => {
            if !attrs.contains_key("xpath") {
                return invalid(format!("{} requires an 'xpath' attribute", local));
            }
        }
        "element" | "attribute" => {
            if !attrs.contains_key("name") && !attrs.contains_key("ref") {
                return invalid(format!("{} requires a 'name' or 'ref' attribute", local));
            }
            if attrs.contains_key("default") && attrs.contains_key("fixed") {
                return invalid(format!("{} cannot have both 'default' and 'fixed'", local));
            }
            if local == "attribute"
                && attrs.contains_key("default")
                && attrs.get("use").is_some_and(|u| u != "optional")
            {
                return invalid(
                    "attribute with a default value must have use='optional'".to_string(),
                );
            }
        }
        _ => {}
    }
    Ok(())
}

/// A reference (ref=) admits no children other than annotation.
fn check_reference_has_no_children(parent: &ChildState, local: &str) -> Result<()> {
    if parent.is_ref
        && matches!(
            parent.name.as_str(),
            "group" | "attributeGroup" | "element" | "attribute"
        )
        && local != "annotation"
    {
        return invalid(format!(
            "'{}' reference cannot have '{local}' children",
            parent.name
        ));
    }
    Ok(())
}

/// Duplicate attribute uses / attributeGroup references in one scope.
fn check_duplicate_attribute_use(
    parent: &mut ChildState,
    local: &str,
    attrs: &HashMap<String, String>,
    attribute_form_qualified: bool,
) -> Result<()> {
    if !matches!(
        parent.name.as_str(),
        "complexType" | "extension" | "restriction" | "attributeGroup"
    ) {
        return Ok(());
    }
    let key = match local {
        "attribute" => {
            if let Some(name) = attrs.get("name") {
                let qualified = match attrs.get("form") {
                    Some(f) => f == "qualified",
                    None => attribute_form_qualified,
                };
                Some(format!("attr:{qualified}:{name}"))
            } else {
                attrs.get("ref").map(|r| format!("attrref:{}", r.trim()))
            }
        }
        "attributeGroup" => attrs.get("ref").map(|r| format!("agref:{}", r.trim())),
        _ => None,
    };
    if let Some(key) = key
        && !parent.attr_keys.insert(key)
    {
        return invalid(format!(
            "duplicate attribute declaration in '{}'",
            parent.name
        ));
    }
    Ok(())
}

/// At most one anyAttribute per scope.
fn check_any_attribute_count(parent: &mut ChildState, local: &str) -> Result<()> {
    if local == "anyAttribute" {
        if parent.any_attributes > 0 {
            return invalid(format!(
                "at most one anyAttribute is allowed in '{}'",
                parent.name
            ));
        }
        parent.any_attributes += 1;
    }
    Ok(())
}

/// Which children each parent kind admits, and how many.
fn check_allowed_child(
    parent: &mut ChildState,
    local: &str,
    attrs: &HashMap<String, String>,
) -> Result<()> {
    match parent.name.as_str() {
        "complexType" => check_complex_type_child(parent, local),
        "simpleContent" | "complexContent" => check_content_derivation_child(parent, local),
        "extension" | "restriction" => check_derivation_particle_child(parent, local),
        "unique" | "key" | "keyref" => check_identity_constraint_child(parent, local),
        // Elements whose only legal child is annotation.
        "notation" | "import" | "include" | "selector" | "field" | "any" | "anyAttribute" => {
            if local != "annotation" {
                return not_allowed_inside(local, &parent.name);
            }
            Ok(())
        }
        // cos-all-limited: xs:all must be the whole content model, so it
        // cannot be nested inside a sequence or choice.
        "sequence" | "choice" => {
            if local == "all" {
                return invalid(format!("'all' cannot be nested inside {}", parent.name));
            }
            Ok(())
        }
        "group" => check_model_group_child(parent, local),
        // An attribute group admits attribute uses and wildcards only.
        "attributeGroup" => {
            if !matches!(
                local,
                "attribute" | "attributeGroup" | "anyAttribute" | "annotation"
            ) {
                return not_allowed_inside(local, "attributeGroup");
            }
            Ok(())
        }
        "element" => check_element_child(parent, local),
        "attribute" => check_attribute_child(parent, local),
        "simpleType" => check_simple_type_child(parent, local),
        "all" => check_all_child(local, attrs),
        _ => Ok(()),
    }
}

/// complexType admits one of: simpleContent | complexContent |
/// one particle (plus attributes etc.).
fn check_complex_type_child(parent: &mut ChildState, local: &str) -> Result<()> {
    if matches!(local, "simpleContent" | "complexContent") {
        // children_seen already includes this child
        if parent.derivations > 0 || parent.particles > 0 || parent.children_seen > 1 {
            return invalid(format!(
                "complexType with {} content cannot have other children",
                local
            ));
        }
        parent.derivations += 1;
    } else if is_particle(local) {
        if parent.derivations > 0 || parent.particles > 0 {
            return invalid(format!(
                "complexType allows only one content child, found extra '{}'",
                local
            ));
        }
        parent.particles += 1;
    } else if local != "annotation" && parent.derivations > 0 {
        // simpleContent/complexContent must be the only child;
        // attributes belong inside the derivation.
        return invalid(format!(
            "complexType with simpleContent/complexContent cannot also contain '{}'",
            local
        ));
    }
    Ok(())
}

/// simpleContent / complexContent admit exactly one
/// restriction/extension (plus annotation).
fn check_content_derivation_child(parent: &mut ChildState, local: &str) -> Result<()> {
    if matches!(local, "restriction" | "extension") {
        if parent.derivations > 0 {
            return invalid(format!("{} allows only one derivation child", parent.name));
        }
        parent.derivations += 1;
    } else if local != "annotation" {
        return not_allowed_inside(local, &parent.name);
    }
    Ok(())
}

/// A complex-content extension/restriction admits at most one
/// particle.
fn check_derivation_particle_child(parent: &mut ChildState, local: &str) -> Result<()> {
    if is_particle(local) {
        if parent.particles > 0 {
            return invalid(format!(
                "{} allows only one particle child, found extra '{}'",
                parent.name, local
            ));
        }
        parent.particles += 1;
    }
    Ok(())
}

/// Identity constraints admit annotation, one selector, fields.
fn check_identity_constraint_child(parent: &mut ChildState, local: &str) -> Result<()> {
    match local {
        "selector" => {
            if parent.selectors > 0 {
                return invalid("only one selector is allowed".to_string());
            }
            parent.selectors += 1;
        }
        "field" => {
            if parent.selectors == 0 {
                return invalid("selector must precede field".to_string());
            }
            parent.fields += 1;
        }
        "annotation" => {}
        _ => return not_allowed_inside(local, &parent.name),
    }
    Ok(())
}

/// A named model group definition admits exactly one particle.
fn check_model_group_child(parent: &mut ChildState, local: &str) -> Result<()> {
    match local {
        "all" | "choice" | "sequence" => {
            if parent.particles > 0 {
                return invalid("group allows only one particle child".to_string());
            }
            parent.particles += 1;
        }
        "annotation" => {}
        _ => return not_allowed_inside(local, "group"),
    }
    Ok(())
}

/// element admits (annotation?, (simpleType|complexType)?, IC*).
fn check_element_child(parent: &mut ChildState, local: &str) -> Result<()> {
    match local {
        "simpleType" | "complexType" => {
            if parent.has_type {
                return invalid(format!(
                    "element with a 'type' attribute cannot have an inline {local}"
                ));
            }
            if parent.derivations > 0 {
                return invalid("element allows only one inline type".to_string());
            }
            parent.derivations += 1;
        }
        "annotation" | "unique" | "key" | "keyref" => {}
        _ => return not_allowed_inside(local, "element"),
    }
    Ok(())
}

/// attribute admits (annotation?, simpleType?).
fn check_attribute_child(parent: &mut ChildState, local: &str) -> Result<()> {
    match local {
        "simpleType" => {
            if parent.has_type {
                return invalid(
                    "attribute with a 'type' attribute cannot have an inline simpleType"
                        .to_string(),
                );
            }
            if parent.derivations > 0 {
                return invalid("attribute allows only one inline simpleType".to_string());
            }
            parent.derivations += 1;
        }
        "annotation" => {}
        _ => return not_allowed_inside(local, "attribute"),
    }
    Ok(())
}

/// simpleType admits exactly one of restriction | list | union.
fn check_simple_type_child(parent: &mut ChildState, local: &str) -> Result<()> {
    match local {
        "restriction" | "list" | "union" => {
            if parent.derivations > 0 {
                return invalid("simpleType allows only one derivation child".to_string());
            }
            parent.derivations += 1;
        }
        "annotation" => {}
        _ => return not_allowed_inside(local, "simpleType"),
    }
    Ok(())
}

/// xs:all admits only element declarations (and annotation), and
/// its member elements must have minOccurs/maxOccurs of 0 or 1.
fn check_all_child(local: &str, attrs: &HashMap<String, String>) -> Result<()> {
    match local {
        "annotation" => {}
        "element" => {
            for occurs_attr in ["minOccurs", "maxOccurs"] {
                if let Some(v) = attrs.get(occurs_attr) {
                    let v = v.trim();
                    // Numerically 0 or 1 ("unbounded" and >1 are out);
                    // unparseable values are rejected by parse_occurs.
                    if v.parse::<u64>().map(|n| n > 1).unwrap_or(v == "unbounded") {
                        return invalid(format!(
                            "element inside 'all' must have {} of 0 or 1, found '{}'",
                            occurs_attr, v
                        ));
                    }
                }
            }
        }
        // Not `not_allowed_inside`: this message quotes 'all'.
        _ => return invalid(format!("element '{}' is not allowed inside 'all'", local)),
    }
    Ok(())
}
