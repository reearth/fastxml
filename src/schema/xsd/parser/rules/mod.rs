//! Schema-for-schemas structural and content-model rules enforced while
//! parsing an XSD document, plus XML Schema versioning (`vc:`) pruning.
//!
//! Split out of `handlers.rs`: these methods judge whether an element (with
//! its attributes) is admissible at its position; the handlers in
//! `handlers.rs` build the AST for admissible elements. The attribute-value
//! rules live in `attribute_values.rs` and the content-model rules in
//! `content_model.rs`.

mod allowed_attributes;
mod attribute_values;
mod content_model;

use std::collections::HashMap;

use crate::error::Result;
use crate::schema::xsd::types::XsdFacet;

use super::helpers::XSD_NAMESPACE;
use super::stack_frame::StackFrame;
use super::{ChildState, XsdParser};

impl XsdParser {
    /// Whether XML Schema versioning (`vc:`) attributes on this element
    /// exclude it for this processor (XSD 1.0 semantics with fastxml's
    /// built-in datatype set). Excluded elements are pruned like annotation
    /// content, per the W3C XML Schema versioning recommendation.
    pub(super) fn pruned_by_versioning(&self, attrs: &[(&str, &str)]) -> bool {
        const VC_NS: &str = "http://www.w3.org/2007/XMLSchema-versioning";
        /// Facets fastxml implements.
        const AVAILABLE_FACETS: &[&str] = &[
            "length",
            "minLength",
            "maxLength",
            "pattern",
            "enumeration",
            "whiteSpace",
            "maxInclusive",
            "maxExclusive",
            "minInclusive",
            "minExclusive",
            "totalDigits",
            "fractionDigits",
            "explicitTimezone",
        ];
        /// The XSD version this processor implements.
        const OUR_VERSION: f64 = 1.0;

        let type_available = |qname: &str| -> bool {
            let (prefix, local) = match qname.split_once(':') {
                Some((p, l)) => (Some(p), l),
                None => (None, qname),
            };
            self.in_scope_namespace(prefix.unwrap_or("")) == Some(XSD_NAMESPACE)
                && crate::schema::xsd::builtin::is_builtin_xsd_type_local(local)
        };

        for (raw_name, value) in attrs {
            let Some((prefix, local)) = raw_name.split_once(':') else {
                continue;
            };
            if self.in_scope_namespace(prefix) != Some(VC_NS) {
                continue;
            }
            let prune = match local {
                // Retained iff minVersion <= V < maxVersion.
                "minVersion" => value.trim().parse::<f64>().is_ok_and(|v| v > OUR_VERSION),
                "maxVersion" => value.trim().parse::<f64>().is_ok_and(|v| v <= OUR_VERSION),
                // Retained iff every listed type is available.
                "typeAvailable" => !value.split_whitespace().all(type_available),
                // Retained iff every listed type is unavailable.
                "typeUnavailable" => value.split_whitespace().any(type_available),
                // Retained iff every listed facet is available.
                "facetAvailable" => !value.split_whitespace().all(|f| {
                    let local = f.split_once(':').map_or(f, |(_, l)| l);
                    AVAILABLE_FACETS.contains(&local)
                }),
                // Retained iff every listed facet is unavailable.
                "facetUnavailable" => value.split_whitespace().any(|f| {
                    let local = f.split_once(':').map_or(f, |(_, l)| l);
                    AVAILABLE_FACETS.contains(&local)
                }),
                _ => false,
            };
            if prune {
                return true;
            }
        }
        false
    }

    /// Enforces schema-for-schemas structural rules at element start:
    /// `id` attribute validity and document-wide uniqueness, annotation
    /// placement (at most one, first child, except under xs:schema /
    /// xs:redefine), and xs:notation placement (top level only).
    pub(super) fn check_structural_rules(
        &mut self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        use crate::schema::error::SchemaError;
        use crate::schema::xsd::primitive::PrimitiveKind;

        // id attributes are xs:ID: valid NCName, unique per document.
        if let Some(id) = attrs.get("id") {
            if PrimitiveKind::Ncname.validate(id).is_err() {
                return Err(SchemaError::InvalidSchema {
                    message: format!("id attribute '{}' is not a valid NCName", id),
                }
                .into());
            }
            if !self.seen_ids.insert(id.clone()) {
                return Err(SchemaError::InvalidSchema {
                    message: format!("duplicate id attribute value '{}'", id),
                }
                .into());
            }
        }

        // Inside a simple-type restriction, only facets / annotation /
        // an inline simpleType are allowed as children.
        if matches!(self.stack.last(), Some(StackFrame::SimpleRestriction(_)))
            && !matches!(
                local,
                "annotation"
                    | "simpleType"
                    | "enumeration"
                    | "pattern"
                    | "length"
                    | "minLength"
                    | "maxLength"
                    | "minInclusive"
                    | "maxInclusive"
                    | "minExclusive"
                    | "maxExclusive"
                    | "totalDigits"
                    | "fractionDigits"
                    | "whiteSpace"
                    | "explicitTimezone"
                    | "assertion" // XSD 1.1 facet; ignored but not rejected
            )
        {
            return Err(SchemaError::InvalidSchema {
                message: format!(
                    "element '{}' is not allowed inside a simple type restriction",
                    local
                ),
            }
            .into());
        }

        // Identity constraints: only allowed inside xs:element, with
        // schema-wide unique NCName names; refer is keyref-only (and
        // required there).
        if matches!(local, "unique" | "key" | "keyref") {
            if !matches!(self.stack.last(), Some(StackFrame::Element(_))) {
                return Err(SchemaError::InvalidSchema {
                    message: format!("'{}' is only allowed inside an element declaration", local),
                }
                .into());
            }
            let Some(name) = attrs.get("name") else {
                return Err(SchemaError::InvalidSchema {
                    message: format!("'{}' requires a 'name' attribute", local),
                }
                .into());
            };
            if crate::schema::xsd::primitive::PrimitiveKind::Ncname
                .validate(name)
                .is_err()
            {
                return Err(SchemaError::InvalidSchema {
                    message: format!("identity constraint name '{}' is not a valid NCName", name),
                }
                .into());
            }
            if !self.seen_constraint_names.insert(name.clone()) {
                return Err(SchemaError::InvalidSchema {
                    message: format!("duplicate identity constraint name '{}'", name),
                }
                .into());
            }
            match (local, attrs.contains_key("refer")) {
                ("keyref", false) => {
                    return Err(SchemaError::InvalidSchema {
                        message: "keyref requires a 'refer' attribute".to_string(),
                    }
                    .into());
                }
                ("unique" | "key", true) => {
                    return Err(SchemaError::InvalidSchema {
                        message: format!("'{}' does not allow a 'refer' attribute", local),
                    }
                    .into());
                }
                _ => {}
            }
        }

        if let Some(parent) = self.child_state_stack.last_mut() {
            let parent_allows_repeat =
                matches!(parent.name.as_str(), "schema" | "redefine" | "override");
            if local == "annotation" {
                if !parent_allows_repeat {
                    if parent.annotations >= 1 {
                        return Err(SchemaError::InvalidSchema {
                            message: format!("multiple annotation elements in '{}'", parent.name),
                        }
                        .into());
                    }
                    if parent.children_seen > 0 {
                        return Err(SchemaError::InvalidSchema {
                            message: format!(
                                "annotation must be the first child of '{}'",
                                parent.name
                            ),
                        }
                        .into());
                    }
                }
                parent.annotations += 1;
            } else {
                if local == "notation" && !parent_allows_repeat {
                    return Err(SchemaError::InvalidSchema {
                        message: "notation is only allowed at the top level of a schema"
                            .to_string(),
                    }
                    .into());
                }
                parent.children_seen += 1;
            }
        }

        self.check_allowed_attributes(local, attrs)?;
        self.check_attribute_value_rules(local, attrs)?;
        self.check_content_model_rules(local, attrs)?;

        self.push_child_state(local, attrs);
        Ok(())
    }

    /// Pushes the child-bookkeeping state for a newly opened XSD element.
    pub(super) fn push_child_state(&mut self, local: &str, attrs: &HashMap<String, String>) {
        self.child_state_stack.push(ChildState {
            name: local.to_string(),
            children_seen: 0,
            annotations: 0,
            particles: 0,
            derivations: 0,
            any_attributes: 0,
            selectors: 0,
            fields: 0,
            is_ref: attrs.contains_key("ref"),
            has_type: attrs.contains_key("type"),
            attr_keys: std::collections::HashSet::new(),
        });
    }

    /// Validates that existing facets in a restriction are consistent with a new facet.
    pub(super) fn validate_facet_consistency(
        facets: &[XsdFacet],
        new_facet: &XsdFacet,
    ) -> Result<()> {
        use crate::schema::error::SchemaError;

        match new_facet {
            XsdFacet::MinLength(min_len) => {
                // Check if maxLength exists and is less than minLength
                for f in facets {
                    if let XsdFacet::MaxLength(max_len) = f {
                        if min_len > max_len {
                            return Err(SchemaError::MinLengthGreaterThanMaxLength {
                                min_length: *min_len as u64,
                                max_length: *max_len as u64,
                            }
                            .into());
                        }
                    }
                }
            }
            XsdFacet::MaxLength(max_len) => {
                // Check if minLength exists and is greater than maxLength
                for f in facets {
                    if let XsdFacet::MinLength(min_len) = f {
                        if min_len > max_len {
                            return Err(SchemaError::MinLengthGreaterThanMaxLength {
                                min_length: *min_len as u64,
                                max_length: *max_len as u64,
                            }
                            .into());
                        }
                    }
                }
            }
            XsdFacet::FractionDigits(frac) => {
                // fractionDigits must be <= totalDigits
                for f in facets {
                    if let XsdFacet::TotalDigits(total) = f {
                        if frac > total {
                            return Err(SchemaError::FractionDigitsGreaterThanTotalDigits {
                                fraction_digits: *frac as u64,
                                total_digits: *total as u64,
                            }
                            .into());
                        }
                    }
                }
            }
            XsdFacet::TotalDigits(total) => {
                // totalDigits must be >= fractionDigits
                for f in facets {
                    if let XsdFacet::FractionDigits(frac) = f {
                        if frac > total {
                            return Err(SchemaError::FractionDigitsGreaterThanTotalDigits {
                                fraction_digits: *frac as u64,
                                total_digits: *total as u64,
                            }
                            .into());
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
