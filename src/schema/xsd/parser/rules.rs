//! Schema-for-schemas structural and content-model rules enforced while
//! parsing an XSD document, plus XML Schema versioning (`vc:`) pruning.
//!
//! Split out of `handlers.rs`: these methods judge whether an element (with
//! its attributes) is admissible at its position; the handlers in
//! `handlers.rs` build the AST for admissible elements.

use std::collections::HashMap;

use crate::error::Result;

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
            let ns = match prefix {
                Some(p) => self.schema.namespace_bindings.get(p).cloned(),
                None => self.schema.namespace_bindings.get("").cloned(),
            };
            ns.as_deref() == Some(XSD_NAMESPACE)
                && crate::schema::xsd::builtin::is_builtin_xsd_type_local(local)
        };

        for (raw_name, value) in attrs {
            let Some((prefix, local)) = raw_name.split_once(':') else {
                continue;
            };
            if self
                .schema
                .namespace_bindings
                .get(prefix)
                .map(String::as_str)
                != Some(VC_NS)
            {
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
            let parent_allows_repeat = parent.name == "schema" || parent.name == "redefine";
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

    /// Enforces lexical and enumeration rules on the attributes of an XSD
    /// element: NCName-valued `name`, QName-valued references, enumerated
    /// values (`form`, `use`, `processContents`, …), boolean attributes,
    /// derivation-control sets, wildcard namespace lists, and the
    /// `ref`-excludes-other-attributes rules.
    pub(super) fn check_attribute_value_rules(
        &self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        use crate::schema::error::SchemaError;
        use crate::schema::xsd::primitive::PrimitiveKind;

        let err =
            |message: String| -> Result<()> { Err(SchemaError::InvalidSchema { message }.into()) };

        // A lexically valid QName: NCName, optionally prefixed.
        let check_qname = |attr: &str| -> Result<()> {
            let Some(value) = attrs.get(attr) else {
                return Ok(());
            };
            // QName attribute values are whitespace-collapsed.
            let value = value.trim();
            let valid = match value.split_once(':') {
                Some((p, l)) => {
                    PrimitiveKind::Ncname.validate(p).is_ok()
                        && PrimitiveKind::Ncname.validate(l).is_ok()
                }
                None => PrimitiveKind::Ncname.validate(value).is_ok(),
            };
            if !valid {
                return Err(SchemaError::InvalidSchema {
                    message: format!("'{value}' is not a valid QName in {local}/@{attr}"),
                }
                .into());
            }
            Ok(())
        };

        // An enumerated attribute value.
        let check_enum = |attr: &str, allowed: &[&str]| -> Result<()> {
            if let Some(value) = attrs.get(attr)
                && !allowed.contains(&value.as_str())
            {
                return Err(SchemaError::InvalidSchema {
                    message: format!("invalid value '{value}' for {local}/@{attr}"),
                }
                .into());
            }
            Ok(())
        };

        // A derivation-control set: '#all' or a subset of `allowed`.
        let check_derivation_set = |attr: &str, allowed: &[&str]| -> Result<()> {
            if let Some(value) = attrs.get(attr) {
                let value = value.trim();
                if value != "#all"
                    && let Some(bad) = value.split_whitespace().find(|t| !allowed.contains(t))
                {
                    return Err(SchemaError::InvalidSchema {
                        message: format!("invalid token '{bad}' in {local}/@{attr}"),
                    }
                    .into());
                }
            }
            Ok(())
        };

        const BOOL: &[&str] = &["true", "false", "1", "0"];

        // NCName-valued name attribute.
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
            return err(format!("'{name}' is not a valid NCName in {local}/@name"));
        }

        match local {
            "element" => {
                check_qname("ref")?;
                check_qname("type")?;
                check_qname("substitutionGroup")?;
                check_enum("form", &["qualified", "unqualified"])?;
                check_enum("abstract", BOOL)?;
                check_enum("nillable", BOOL)?;
                check_derivation_set("block", &["extension", "restriction", "substitution"])?;
                check_derivation_set("final", &["extension", "restriction"])?;
                if attrs.contains_key("ref") {
                    for conflicting in [
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
                    ] {
                        if attrs.contains_key(conflicting) {
                            return err(format!(
                                "element reference cannot also have '{conflicting}'"
                            ));
                        }
                    }
                }
            }
            "attribute" => {
                check_qname("ref")?;
                check_qname("type")?;
                check_enum("form", &["qualified", "unqualified"])?;
                check_enum("use", &["optional", "prohibited", "required"])?;
                // Note: use='prohibited' with fixed= is legal; only default=
                // requires use='optional' (enforced in content-model rules).
                if attrs.contains_key("ref") {
                    for conflicting in ["name", "type", "form"] {
                        if attrs.contains_key(conflicting) {
                            return err(format!(
                                "attribute reference cannot also have '{conflicting}'"
                            ));
                        }
                    }
                }
            }
            "group" | "attributeGroup" => {
                check_qname("ref")?;
            }
            "restriction" | "extension" => {
                check_qname("base")?;
            }
            "list" => {
                check_qname("itemType")?;
            }
            "union" => {
                if let Some(members) = attrs.get("memberTypes") {
                    for member in members.split_whitespace() {
                        let valid = match member.split_once(':') {
                            Some((p, l)) => {
                                PrimitiveKind::Ncname.validate(p).is_ok()
                                    && PrimitiveKind::Ncname.validate(l).is_ok()
                            }
                            None => PrimitiveKind::Ncname.validate(member).is_ok(),
                        };
                        if !valid {
                            return err(format!(
                                "'{member}' is not a valid QName in union/@memberTypes"
                            ));
                        }
                    }
                }
            }
            "keyref" => {
                check_qname("refer")?;
            }
            "complexType" => {
                check_enum("abstract", BOOL)?;
                check_enum("mixed", BOOL)?;
                check_derivation_set("block", &["extension", "restriction"])?;
                check_derivation_set("final", &["extension", "restriction"])?;
            }
            "complexContent" => {
                check_enum("mixed", BOOL)?;
            }
            "simpleType" => {
                check_derivation_set("final", &["list", "union", "restriction"])?;
            }
            "schema" => {
                check_enum("elementFormDefault", &["qualified", "unqualified"])?;
                check_enum("attributeFormDefault", &["qualified", "unqualified"])?;
                check_derivation_set(
                    "blockDefault",
                    &["extension", "restriction", "substitution"],
                )?;
                check_derivation_set(
                    "finalDefault",
                    &["extension", "restriction", "list", "union"],
                )?;
            }
            "any" | "anyAttribute" => {
                check_enum("processContents", &["strict", "lax", "skip"])?;
                if let Some(namespace) = attrs.get("namespace") {
                    let value = namespace.trim();
                    if value != "##any" && value != "##other" {
                        for token in value.split_whitespace() {
                            if token.starts_with("##")
                                && token != "##targetNamespace"
                                && token != "##local"
                            {
                                return err(format!("invalid wildcard namespace token '{token}'"));
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Enforces per-element content rules from the schema for schemas:
    /// which children an XSD element admits, how many, and which attributes
    /// are required.
    pub(super) fn check_content_model_rules(
        &mut self,
        local: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        use crate::schema::error::SchemaError;

        let err =
            |message: String| -> Result<()> { Err(SchemaError::InvalidSchema { message }.into()) };

        // Required attributes on the element itself.
        match local {
            "notation" => {
                // Per the XSD 1.0 errata, at least one of public/system.
                if !attrs.contains_key("name")
                    || (!attrs.contains_key("public") && !attrs.contains_key("system"))
                {
                    return err(
                        "notation requires 'name' and at least one of 'public'/'system'"
                            .to_string(),
                    );
                }
            }
            "selector" | "field" => {
                if !attrs.contains_key("xpath") {
                    return err(format!("{} requires an 'xpath' attribute", local));
                }
            }
            "element" | "attribute" => {
                if !attrs.contains_key("name") && !attrs.contains_key("ref") {
                    return err(format!("{} requires a 'name' or 'ref' attribute", local));
                }
                if attrs.contains_key("default") && attrs.contains_key("fixed") {
                    return err(format!("{} cannot have both 'default' and 'fixed'", local));
                }
                if local == "attribute"
                    && attrs.contains_key("default")
                    && attrs.get("use").is_some_and(|u| u != "optional")
                {
                    return err(
                        "attribute with a default value must have use='optional'".to_string()
                    );
                }
            }
            _ => {}
        }

        let is_particle = matches!(local, "group" | "all" | "choice" | "sequence");

        // xs:schema is only allowed as the document root.
        if local == "schema" && !self.child_state_stack.is_empty() {
            return err("'schema' is only allowed as the document root".to_string());
        }

        let parent_is_top = self
            .child_state_stack
            .last()
            .is_some_and(|p| p.name == "schema" || p.name == "redefine");

        // Named group / attributeGroup definitions are top-level only; uses
        // elsewhere must be references (src-attribute_group / src-model_group).
        if matches!(local, "group" | "attributeGroup") && !self.child_state_stack.is_empty() {
            if parent_is_top {
                if attrs.contains_key("ref") {
                    return err(format!("top-level {local} cannot have 'ref'"));
                }
                if !attrs.contains_key("name") {
                    return err(format!("top-level {local} requires 'name'"));
                }
            } else {
                if attrs.contains_key("name") {
                    return err(format!("nested {local} cannot have 'name'"));
                }
                if !attrs.contains_key("ref") {
                    return err(format!("nested {local} requires 'ref'"));
                }
            }
        }

        // Duplicate top-level component names within one document (simple
        // and complex types share a symbol space).
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
                return err(format!("duplicate top-level {local} '{name}'"));
            }
        }

        let Some(parent) = self.child_state_stack.last_mut() else {
            return Ok(());
        };

        // A reference (ref=) admits no children other than annotation.
        if parent.is_ref
            && matches!(
                parent.name.as_str(),
                "group" | "attributeGroup" | "element" | "attribute"
            )
            && local != "annotation"
        {
            return err(format!(
                "'{}' reference cannot have '{local}' children",
                parent.name
            ));
        }

        // Duplicate attribute uses / attributeGroup references in one scope.
        if matches!(
            parent.name.as_str(),
            "complexType" | "extension" | "restriction" | "attributeGroup"
        ) {
            let key = match local {
                "attribute" => {
                    if let Some(name) = attrs.get("name") {
                        let qualified = match attrs.get("form") {
                            Some(f) => f == "qualified",
                            None => {
                                self.schema.attribute_form_default
                                    == crate::schema::xsd::types::FormDefault::Qualified
                            }
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
                return err(format!(
                    "duplicate attribute declaration in '{}'",
                    parent.name
                ));
            }
        }

        if local == "anyAttribute" {
            if parent.any_attributes > 0 {
                return err(format!(
                    "at most one anyAttribute is allowed in '{}'",
                    parent.name
                ));
            }
            parent.any_attributes += 1;
        }

        match parent.name.as_str() {
            // complexType admits one of: simpleContent | complexContent |
            // one particle (plus attributes etc.).
            "complexType" => {
                if matches!(local, "simpleContent" | "complexContent") {
                    // children_seen already includes this child
                    if parent.derivations > 0 || parent.particles > 0 || parent.children_seen > 1 {
                        return err(format!(
                            "complexType with {} content cannot have other children",
                            local
                        ));
                    }
                    parent.derivations += 1;
                } else if is_particle {
                    if parent.derivations > 0 || parent.particles > 0 {
                        return err(format!(
                            "complexType allows only one content child, found extra '{}'",
                            local
                        ));
                    }
                    parent.particles += 1;
                } else if local != "annotation" && parent.derivations > 0 {
                    // simpleContent/complexContent must be the only child;
                    // attributes belong inside the derivation.
                    return err(format!(
                        "complexType with simpleContent/complexContent cannot also contain '{}'",
                        local
                    ));
                }
            }
            // simpleContent / complexContent admit exactly one
            // restriction/extension (plus annotation).
            "simpleContent" | "complexContent" => {
                if matches!(local, "restriction" | "extension") {
                    if parent.derivations > 0 {
                        return err(format!("{} allows only one derivation child", parent.name));
                    }
                    parent.derivations += 1;
                } else if local != "annotation" {
                    return err(format!(
                        "element '{}' is not allowed inside {}",
                        local, parent.name
                    ));
                }
            }
            // A complex-content extension/restriction admits at most one
            // particle.
            "extension" | "restriction" => {
                if is_particle {
                    if parent.particles > 0 {
                        return err(format!(
                            "{} allows only one particle child, found extra '{}'",
                            parent.name, local
                        ));
                    }
                    parent.particles += 1;
                }
            }
            // Identity constraints admit annotation, one selector, fields.
            "unique" | "key" | "keyref" => match local {
                "selector" => {
                    if parent.selectors > 0 {
                        return err("only one selector is allowed".to_string());
                    }
                    parent.selectors += 1;
                }
                "field" => {
                    if parent.selectors == 0 {
                        return err("selector must precede field".to_string());
                    }
                    parent.fields += 1;
                }
                "annotation" => {}
                _ => {
                    return err(format!(
                        "element '{}' is not allowed inside {}",
                        local, parent.name
                    ));
                }
            },
            // Elements whose only legal child is annotation.
            "notation" | "import" | "include" | "selector" | "field" | "any" | "anyAttribute" => {
                if local != "annotation" {
                    return err(format!(
                        "element '{}' is not allowed inside {}",
                        local, parent.name
                    ));
                }
            }
            // cos-all-limited: xs:all must be the whole content model, so it
            // cannot be nested inside a sequence or choice.
            "sequence" | "choice" => {
                if local == "all" {
                    return err(format!("'all' cannot be nested inside {}", parent.name));
                }
            }
            // A named model group definition admits exactly one particle.
            "group" => match local {
                "all" | "choice" | "sequence" => {
                    if parent.particles > 0 {
                        return err("group allows only one particle child".to_string());
                    }
                    parent.particles += 1;
                }
                "annotation" => {}
                _ => {
                    return err(format!("element '{}' is not allowed inside group", local));
                }
            },
            // An attribute group admits attribute uses and wildcards only.
            "attributeGroup" => {
                if !matches!(
                    local,
                    "attribute" | "attributeGroup" | "anyAttribute" | "annotation"
                ) {
                    return err(format!(
                        "element '{}' is not allowed inside attributeGroup",
                        local
                    ));
                }
            }
            // element admits (annotation?, (simpleType|complexType)?, IC*).
            "element" => match local {
                "simpleType" | "complexType" => {
                    if parent.has_type {
                        return err(format!(
                            "element with a 'type' attribute cannot have an inline {local}"
                        ));
                    }
                    if parent.derivations > 0 {
                        return err("element allows only one inline type".to_string());
                    }
                    parent.derivations += 1;
                }
                "annotation" | "unique" | "key" | "keyref" => {}
                _ => {
                    return err(format!("element '{}' is not allowed inside element", local));
                }
            },
            // attribute admits (annotation?, simpleType?).
            "attribute" => match local {
                "simpleType" => {
                    if parent.has_type {
                        return err(
                            "attribute with a 'type' attribute cannot have an inline simpleType"
                                .to_string(),
                        );
                    }
                    if parent.derivations > 0 {
                        return err("attribute allows only one inline simpleType".to_string());
                    }
                    parent.derivations += 1;
                }
                "annotation" => {}
                _ => {
                    return err(format!(
                        "element '{}' is not allowed inside attribute",
                        local
                    ));
                }
            },
            // simpleType admits exactly one of restriction | list | union.
            "simpleType" => match local {
                "restriction" | "list" | "union" => {
                    if parent.derivations > 0 {
                        return err("simpleType allows only one derivation child".to_string());
                    }
                    parent.derivations += 1;
                }
                "annotation" => {}
                _ => {
                    return err(format!(
                        "element '{}' is not allowed inside simpleType",
                        local
                    ));
                }
            },
            // xs:all admits only element declarations (and annotation), and
            // its member elements must have minOccurs/maxOccurs of 0 or 1.
            "all" => match local {
                "annotation" => {}
                "element" => {
                    for occurs_attr in ["minOccurs", "maxOccurs"] {
                        if let Some(v) = attrs.get(occurs_attr) {
                            let v = v.trim();
                            // Numerically 0 or 1 ("unbounded" and >1 are out);
                            // unparseable values are rejected by parse_occurs.
                            if v.parse::<u64>().map(|n| n > 1).unwrap_or(v == "unbounded") {
                                return err(format!(
                                    "element inside 'all' must have {} of 0 or 1, found '{}'",
                                    occurs_attr, v
                                ));
                            }
                        }
                    }
                }
                _ => {
                    return err(format!("element '{}' is not allowed inside 'all'", local));
                }
            },
            _ => {}
        }

        Ok(())
    }
}
