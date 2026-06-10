//! Handler functions for XSD parsing.

use std::collections::HashMap;

use crate::error::Result;
use crate::schema::xsd::types::*;

use super::XsdParser;
use super::helpers::{XSD_NAMESPACE, parse_occurs};
use super::stack_frame::StackFrame;

impl XsdParser {
    /// Handles a start element event.
    pub(super) fn handle_start(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        attrs: &[(&str, &str)],
        namespace_decls: &[crate::namespace::Namespace],
    ) -> Result<()> {
        // Track the default namespace in scope for this element (an
        // `xmlns="..."` declaration overrides the inherited one).
        let default_ns = namespace_decls
            .iter()
            .find(|ns| ns.prefix().is_empty())
            .map(|ns| Some(ns.uri().to_string()))
            .unwrap_or_else(|| self.default_ns_stack.last().cloned().flatten());
        self.default_ns_stack.push(default_ns);

        // Check for XSD namespace binding (only if not yet detected)
        if self.xsd_prefix.is_none() {
            // First pass: look for default namespace (empty prefix) - preferred
            let mut prefixed_binding: Option<String> = None;
            for ns in namespace_decls {
                if ns.uri() == XSD_NAMESPACE {
                    if ns.prefix().is_empty() {
                        // Default namespace is XSD (no prefix) - this is preferred
                        self.xsd_prefix = Some(None);
                        break;
                    } else if prefixed_binding.is_none() {
                        // Remember the prefixed binding as fallback
                        prefixed_binding = Some(ns.prefix().to_string());
                    }
                }
            }
            // If no default namespace, use the prefixed binding
            if self.xsd_prefix.is_none() {
                if let Some(prefix) = prefixed_binding {
                    self.xsd_prefix = Some(Some(prefix));
                }
            }
        }

        // Store all namespace bindings
        for ns in namespace_decls {
            self.schema
                .namespace_bindings
                .insert(ns.prefix().to_string(), ns.uri().to_string());
        }

        // Skip annotation content
        if self.skip_depth > 0 {
            self.skip_depth += 1;
            return Ok(());
        }

        if !self.is_xsd_element(name, prefix) {
            // Not an XSD element, skip
            return Ok(());
        }

        let local = self.xsd_local_name(name);
        let attr_map = super::helpers::parse_attributes(attrs);

        match local {
            "schema" => {
                self.handle_schema(&attr_map)?;
            }
            "element" => {
                self.handle_element(&attr_map)?;
            }
            "complexType" => {
                self.handle_complex_type(&attr_map)?;
            }
            "simpleType" => {
                self.handle_simple_type(&attr_map)?;
            }
            "sequence" => {
                self.handle_sequence(&attr_map)?;
            }
            "choice" => {
                self.handle_choice(&attr_map)?;
            }
            "all" => {
                self.handle_all(&attr_map)?;
            }
            "attribute" => {
                self.handle_attribute(&attr_map)?;
            }
            "attributeGroup" => {
                self.handle_attribute_group(&attr_map)?;
            }
            "group" => {
                self.handle_group(&attr_map)?;
            }
            "restriction" => {
                self.handle_restriction(&attr_map)?;
            }
            "extension" => {
                self.handle_extension(&attr_map)?;
            }
            "complexContent" => {
                self.handle_complex_content(&attr_map)?;
            }
            "simpleContent" => {
                self.handle_simple_content(&attr_map)?;
            }
            "import" => {
                self.handle_import(&attr_map)?;
            }
            "include" => {
                self.handle_include(&attr_map)?;
            }
            "redefine" => {
                self.handle_redefine(&attr_map)?;
            }
            "unique" => {
                self.handle_unique(&attr_map)?;
            }
            "key" => {
                self.handle_key(&attr_map)?;
            }
            "keyref" => {
                self.handle_keyref(&attr_map)?;
            }
            "selector" => {
                self.handle_selector(&attr_map)?;
            }
            "field" => {
                self.handle_field(&attr_map)?;
            }
            "list" => {
                self.handle_list(&attr_map)?;
            }
            "union" => {
                self.handle_union(&attr_map)?;
            }
            "any" => {
                self.handle_any(&attr_map)?;
            }
            "anyAttribute" => {
                self.stack.push(StackFrame::AnyAttribute);
            }
            // Facets
            "enumeration" | "pattern" | "minLength" | "maxLength" | "length" | "minInclusive"
            | "maxInclusive" | "minExclusive" | "maxExclusive" | "totalDigits"
            | "fractionDigits" | "whiteSpace" => {
                self.handle_facet(local, &attr_map)?;
            }
            // Annotation elements (skip content)
            "annotation" => {
                self.stack.push(StackFrame::Annotation);
                self.skip_depth = 1;
            }
            "documentation" => {
                self.stack.push(StackFrame::Documentation);
            }
            "appinfo" => {
                self.stack.push(StackFrame::AppInfo);
            }
            _ => {
                // Unknown element, ignore
            }
        }

        Ok(())
    }

    pub(super) fn handle_schema(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        if let Some(tns) = attrs.get("targetNamespace") {
            self.schema.target_namespace = Some(tns.clone());
        }
        if let Some(efd) = attrs.get("elementFormDefault") {
            self.schema.element_form_default = match efd.as_str() {
                "qualified" => FormDefault::Qualified,
                _ => FormDefault::Unqualified,
            };
        }
        if let Some(afd) = attrs.get("attributeFormDefault") {
            self.schema.attribute_form_default = match afd.as_str() {
                "qualified" => FormDefault::Qualified,
                _ => FormDefault::Unqualified,
            };
        }
        if let Some(v) = attrs.get("version") {
            self.schema.version = Some(v.clone());
        }
        self.stack.push(StackFrame::Schema);
        Ok(())
    }

    pub(super) fn handle_element(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut elem = if let Some(ref_attr) = attrs.get("ref") {
            XsdElement::ref_(QName::parse(ref_attr))
        } else {
            XsdElement::new(attrs.get("name").cloned().unwrap_or_default())
        };

        if let Some(type_attr) = attrs.get("type") {
            elem.type_ref = Some(QName::parse(type_attr));
        }

        // Parse and validate minOccurs/maxOccurs
        let (min, max) = parse_occurs(attrs)?;
        elem.min_occurs = min;
        elem.max_occurs = max;
        if attrs.get("abstract").is_some_and(|v| v == "true") {
            elem.is_abstract = true;
        }
        if let Some(sg) = attrs.get("substitutionGroup") {
            elem.substitution_group = Some(QName::parse(sg));
        }
        if attrs.get("nillable").is_some_and(|v| v == "true") {
            elem.nillable = true;
        }
        if let Some(def) = attrs.get("default") {
            elem.default = Some(def.clone());
        }
        if let Some(fix) = attrs.get("fixed") {
            elem.fixed = Some(fix.clone());
        }
        if let Some(form) = attrs.get("form") {
            elem.form = Some(match form.as_str() {
                "qualified" => FormDefault::Qualified,
                _ => FormDefault::Unqualified,
            });
        }

        self.stack.push(StackFrame::Element(elem));
        Ok(())
    }

    pub(super) fn handle_complex_type(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut ct = if let Some(name) = attrs.get("name") {
            XsdComplexType::new(name)
        } else {
            XsdComplexType::anonymous()
        };

        if attrs.get("abstract").is_some_and(|v| v == "true") {
            ct.is_abstract = true;
        }
        if attrs.get("mixed").is_some_and(|v| v == "true") {
            ct.mixed = true;
        }

        self.stack.push(StackFrame::ComplexType(ct));
        Ok(())
    }

    pub(super) fn handle_simple_type(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let st = if let Some(name) = attrs.get("name") {
            XsdSimpleType::new(name)
        } else {
            XsdSimpleType::anonymous()
        };

        self.stack.push(StackFrame::SimpleType(st));
        Ok(())
    }

    pub(super) fn handle_sequence(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut seq = XsdSequence::default();
        let (min, max) = parse_occurs(attrs)?;
        seq.min_occurs = min;
        seq.max_occurs = max;
        self.stack.push(StackFrame::Sequence(seq));
        Ok(())
    }

    pub(super) fn handle_choice(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut choice = XsdChoice::default();
        let (min, max) = parse_occurs(attrs)?;
        choice.min_occurs = min;
        choice.max_occurs = max;
        self.stack.push(StackFrame::Choice(choice));
        Ok(())
    }

    pub(super) fn handle_all(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        use crate::schema::error::SchemaError;

        let mut all = XsdAll::default();
        // Parse minOccurs (maxOccurs for all is always 1 per XSD spec)
        if let Some(min_str) = attrs.get("minOccurs") {
            all.min_occurs =
                Occurs::parse(min_str).map_err(|e| SchemaError::InvalidOccurs { value: e })?;
        }
        all.max_occurs = Occurs::Count(1);
        self.stack.push(StackFrame::All(all));
        Ok(())
    }

    pub(super) fn handle_attribute(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut attr = if let Some(ref_attr) = attrs.get("ref") {
            XsdAttribute::ref_(QName::parse(ref_attr))
        } else {
            XsdAttribute::new(attrs.get("name").cloned().unwrap_or_default())
        };

        if let Some(type_attr) = attrs.get("type") {
            attr.type_ref = Some(QName::parse(type_attr));
        }
        if let Some(use_attr) = attrs.get("use") {
            attr.use_ = match use_attr.as_str() {
                "required" => AttributeUse::Required,
                "prohibited" => AttributeUse::Prohibited,
                _ => AttributeUse::Optional,
            };
        }
        if let Some(def) = attrs.get("default") {
            attr.default = Some(def.clone());
        }
        if let Some(fix) = attrs.get("fixed") {
            attr.fixed = Some(fix.clone());
        }
        if let Some(form) = attrs.get("form") {
            attr.form = Some(match form.as_str() {
                "qualified" => FormDefault::Qualified,
                _ => FormDefault::Unqualified,
            });
        }

        self.stack.push(StackFrame::Attribute(attr));
        Ok(())
    }

    pub(super) fn handle_attribute_group(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let ag = if let Some(ref_attr) = attrs.get("ref") {
            XsdAttributeGroup::ref_(QName::parse(ref_attr))
        } else {
            XsdAttributeGroup::new(attrs.get("name").cloned().unwrap_or_default())
        };
        self.stack.push(StackFrame::AttributeGroup(ag));
        Ok(())
    }

    pub(super) fn handle_group(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut grp = if let Some(ref_attr) = attrs.get("ref") {
            XsdGroup::ref_(QName::parse(ref_attr))
        } else {
            XsdGroup::new(attrs.get("name").cloned().unwrap_or_default())
        };

        let (min, max) = parse_occurs(attrs)?;
        grp.min_occurs = min;
        grp.max_occurs = max;

        self.stack.push(StackFrame::Group(grp));
        Ok(())
    }

    pub(super) fn handle_restriction(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let base = attrs.get("base").map(|s| QName::parse(s));

        // Determine context from stack
        let parent_is_simple_content = self
            .stack
            .iter()
            .rev()
            .any(|f| matches!(f, StackFrame::SimpleContent));
        let parent_is_complex_content = self
            .stack
            .iter()
            .rev()
            .any(|f| matches!(f, StackFrame::ComplexContent { .. }));

        if parent_is_complex_content {
            let restriction = XsdComplexContentRestriction {
                base: base.unwrap_or_else(|| QName::new("")),
                particle: None,
                attributes: Vec::new(),
                attribute_groups: Vec::new(),
            };
            self.stack
                .push(StackFrame::ComplexContentRestriction(restriction));
        } else if parent_is_simple_content {
            let restriction = XsdSimpleContentRestriction {
                base: base.unwrap_or_else(|| QName::new("")),
                facets: Vec::new(),
                attributes: Vec::new(),
                attribute_groups: Vec::new(),
            };
            self.stack
                .push(StackFrame::SimpleContentRestriction(restriction));
        } else {
            // Simple type restriction
            let restriction = XsdSimpleRestriction {
                base,
                inline_base: None,
                facets: Vec::new(),
            };
            self.stack.push(StackFrame::SimpleRestriction(restriction));
        }

        Ok(())
    }

    pub(super) fn handle_extension(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let base = attrs
            .get("base")
            .map(|s| QName::parse(s))
            .unwrap_or_else(|| QName::new(""));

        // Determine context from stack
        let parent_is_simple_content = self
            .stack
            .iter()
            .rev()
            .any(|f| matches!(f, StackFrame::SimpleContent));

        if parent_is_simple_content {
            let extension = XsdSimpleContentExtension {
                base,
                attributes: Vec::new(),
                attribute_groups: Vec::new(),
            };
            self.stack
                .push(StackFrame::SimpleContentExtension(extension));
        } else {
            // Complex content extension
            let extension = XsdComplexContentExtension {
                base,
                particle: None,
                attributes: Vec::new(),
                attribute_groups: Vec::new(),
            };
            self.stack
                .push(StackFrame::ComplexContentExtension(extension));
        }

        Ok(())
    }

    pub(super) fn handle_complex_content(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mixed = attrs.get("mixed").is_some_and(|v| v == "true");
        self.stack.push(StackFrame::ComplexContent { mixed });
        Ok(())
    }

    pub(super) fn handle_simple_content(&mut self, _attrs: &HashMap<String, String>) -> Result<()> {
        self.stack.push(StackFrame::SimpleContent);
        Ok(())
    }

    pub(super) fn handle_import(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let import = XsdImport {
            namespace: attrs.get("namespace").cloned(),
            schema_location: attrs.get("schemaLocation").cloned(),
        };
        self.schema.imports.push(import);
        Ok(())
    }

    pub(super) fn handle_include(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        if let Some(loc) = attrs.get("schemaLocation") {
            let include = XsdInclude {
                schema_location: loc.clone(),
            };
            self.schema.includes.push(include);
        }
        Ok(())
    }

    pub(super) fn handle_redefine(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let schema_location = attrs.get("schemaLocation").cloned().unwrap_or_default();
        let redefine = XsdRedefine::new(schema_location);
        self.stack.push(StackFrame::Redefine(redefine));
        Ok(())
    }

    pub(super) fn handle_unique(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        // Selector will be set when we encounter the selector element
        let constraint = XsdIdentityConstraint::unique(name, "");
        self.stack.push(StackFrame::Unique(constraint));
        Ok(())
    }

    pub(super) fn handle_key(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        let constraint = XsdIdentityConstraint::key(name, "");
        self.stack.push(StackFrame::Key(constraint));
        Ok(())
    }

    pub(super) fn handle_keyref(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        let refer = attrs
            .get("refer")
            .map(|s| QName::parse(s))
            .unwrap_or_else(|| QName::new(""));
        let constraint = XsdIdentityConstraint::keyref(name, "", refer);
        self.stack.push(StackFrame::KeyRef(constraint));
        Ok(())
    }

    pub(super) fn handle_selector(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let xpath = attrs.get("xpath").cloned().unwrap_or_default();

        // Set the selector on the parent constraint
        for frame in self.stack.iter_mut().rev() {
            match frame {
                StackFrame::Unique(c) | StackFrame::Key(c) | StackFrame::KeyRef(c) => {
                    c.selector = xpath;
                    break;
                }
                _ => continue,
            }
        }
        Ok(())
    }

    pub(super) fn handle_field(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let xpath = attrs.get("xpath").cloned().unwrap_or_default();

        // Add the field to the parent constraint
        for frame in self.stack.iter_mut().rev() {
            match frame {
                StackFrame::Unique(c) | StackFrame::Key(c) | StackFrame::KeyRef(c) => {
                    c.fields.push(xpath);
                    break;
                }
                _ => continue,
            }
        }
        Ok(())
    }

    pub(super) fn handle_list(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let list = XsdSimpleList {
            item_type: attrs.get("itemType").map(|s| QName::parse(s)),
            inline_type: None,
        };
        self.stack.push(StackFrame::SimpleList(list));
        Ok(())
    }

    pub(super) fn handle_union(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let member_types = attrs
            .get("memberTypes")
            .map(|s| s.split_whitespace().map(QName::parse).collect())
            .unwrap_or_default();
        let union = XsdSimpleUnion {
            member_types,
            inline_types: Vec::new(),
        };
        self.stack.push(StackFrame::SimpleUnion(union));
        Ok(())
    }

    pub(super) fn handle_any(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut any = XsdAny::default();

        let (min, max) = parse_occurs(attrs)?;
        any.min_occurs = min;
        any.max_occurs = max;
        if let Some(ns) = attrs.get("namespace") {
            any.namespace = match ns.as_str() {
                "##any" => NamespaceConstraint::Any,
                "##other" => NamespaceConstraint::Other,
                "##targetNamespace" => NamespaceConstraint::TargetNamespace,
                "##local" => NamespaceConstraint::Local,
                _ => NamespaceConstraint::List(ns.split_whitespace().map(String::from).collect()),
            };
        }
        if let Some(pc) = attrs.get("processContents") {
            any.process_contents = match pc.as_str() {
                "lax" => ProcessContentsMode::Lax,
                "skip" => ProcessContentsMode::Skip,
                _ => ProcessContentsMode::Strict,
            };
        }

        self.stack.push(StackFrame::Any(any));
        Ok(())
    }

    /// Validates that existing facets in a restriction are consistent with a new facet.
    fn validate_facet_consistency(facets: &[XsdFacet], new_facet: &XsdFacet) -> Result<()> {
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

    pub(super) fn handle_facet(
        &mut self,
        name: &str,
        attrs: &HashMap<String, String>,
    ) -> Result<()> {
        let value = attrs.get("value").cloned().unwrap_or_default();

        let facet = match name {
            "enumeration" => XsdFacet::Enumeration(value),
            "pattern" => XsdFacet::Pattern(value),
            "minLength" => {
                XsdFacet::MinLength(super::helpers::parse_facet_length("minLength", &value)?)
            }
            "maxLength" => {
                XsdFacet::MaxLength(super::helpers::parse_facet_length("maxLength", &value)?)
            }
            "length" => XsdFacet::Length(super::helpers::parse_facet_length("length", &value)?),
            "minInclusive" => XsdFacet::MinInclusive(value),
            "maxInclusive" => XsdFacet::MaxInclusive(value),
            "minExclusive" => XsdFacet::MinExclusive(value),
            "maxExclusive" => XsdFacet::MaxExclusive(value),
            "totalDigits" => {
                XsdFacet::TotalDigits(super::helpers::parse_facet_length("totalDigits", &value)?)
            }
            "fractionDigits" => XsdFacet::FractionDigits(super::helpers::parse_facet_length(
                "fractionDigits",
                &value,
            )?),
            "whiteSpace" => XsdFacet::WhiteSpace(match value.as_str() {
                "preserve" => WhiteSpaceValue::Preserve,
                "replace" => WhiteSpaceValue::Replace,
                _ => WhiteSpaceValue::Collapse,
            }),
            _ => return Ok(()),
        };

        // Add facet to the appropriate restriction with consistency validation
        for frame in self.stack.iter_mut().rev() {
            match frame {
                StackFrame::SimpleRestriction(r) => {
                    Self::validate_facet_consistency(&r.facets, &facet)?;
                    r.facets.push(facet);
                    break;
                }
                StackFrame::SimpleContentRestriction(r) => {
                    Self::validate_facet_consistency(&r.facets, &facet)?;
                    r.facets.push(facet);
                    break;
                }
                _ => continue,
            }
        }

        Ok(())
    }

    /// Handles an end element event.
    pub(super) fn handle_end(&mut self, name: &str, prefix: Option<&str>) -> Result<()> {
        // Pop this element's default-namespace scope (pushed in handle_start)
        // and use it to judge whether this is an XSD element.
        let ending_default_ns = self.default_ns_stack.pop().flatten();

        // Handle annotation skipping
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            if self.skip_depth == 0 {
                // Pop the annotation frame
                self.stack.pop();
            }
            return Ok(());
        }

        let is_xsd = match (&prefix, &ending_default_ns) {
            (None, Some(uri)) => uri == XSD_NAMESPACE,
            _ => self.is_xsd_element(name, prefix),
        };
        if !is_xsd {
            return Ok(());
        }

        let local = self.xsd_local_name(name);

        // Pop annotation frames without processing
        if matches!(local, "documentation" | "appinfo") {
            self.stack.pop();
            return Ok(());
        }

        // Facets don't push stack frames, so skip them
        if matches!(
            local,
            "enumeration"
                | "pattern"
                | "minLength"
                | "maxLength"
                | "length"
                | "minInclusive"
                | "maxInclusive"
                | "minExclusive"
                | "maxExclusive"
                | "totalDigits"
                | "fractionDigits"
                | "whiteSpace"
        ) {
            return Ok(());
        }

        // Also skip import/include/selector/field as they don't push frames
        if matches!(local, "import" | "include" | "selector" | "field") {
            return Ok(());
        }

        let frame = match self.stack.pop() {
            Some(f) => f,
            None => return Ok(()),
        };

        match frame {
            StackFrame::Schema => {
                // Schema end - nothing to do
            }
            StackFrame::Element(elem) => {
                self.finish_element(elem)?;
            }
            StackFrame::ComplexType(ct) => {
                self.finish_complex_type(ct)?;
            }
            StackFrame::SimpleType(st) => {
                self.finish_simple_type(st)?;
            }
            StackFrame::Sequence(seq) => {
                self.finish_sequence(seq)?;
            }
            StackFrame::Choice(choice) => {
                self.finish_choice(choice)?;
            }
            StackFrame::All(all) => {
                self.finish_all(all)?;
            }
            StackFrame::Attribute(attr) => {
                self.finish_attribute(attr)?;
            }
            StackFrame::AttributeGroup(ag) => {
                self.finish_attribute_group(ag)?;
            }
            StackFrame::Group(grp) => {
                self.finish_group(grp)?;
            }
            StackFrame::SimpleRestriction(r) => {
                self.finish_simple_restriction(r)?;
            }
            StackFrame::SimpleContentExtension(ext) => {
                self.finish_simple_content_extension(ext)?;
            }
            StackFrame::SimpleContentRestriction(r) => {
                self.finish_simple_content_restriction(r)?;
            }
            StackFrame::ComplexContent { mixed } => {
                self.finish_complex_content(mixed)?;
            }
            StackFrame::SimpleContent => {
                // Nothing to do - derivation was already processed
            }
            StackFrame::ComplexContentExtension(ext) => {
                self.finish_complex_content_extension(ext)?;
            }
            StackFrame::ComplexContentRestriction(r) => {
                self.finish_complex_content_restriction(r)?;
            }
            StackFrame::SimpleList(list) => {
                self.finish_simple_list(list)?;
            }
            StackFrame::SimpleUnion(union) => {
                self.finish_simple_union(union)?;
            }
            StackFrame::Any(any) => {
                self.finish_any(any)?;
            }
            StackFrame::AnyAttribute => {
                // Ignored
            }
            StackFrame::Annotation | StackFrame::Documentation | StackFrame::AppInfo => {
                // Skip
            }
            StackFrame::Unique(constraint) => {
                self.finish_identity_constraint(constraint)?;
            }
            StackFrame::Key(constraint) => {
                self.finish_identity_constraint(constraint)?;
            }
            StackFrame::KeyRef(constraint) => {
                self.finish_identity_constraint(constraint)?;
            }
            StackFrame::Redefine(redefine) => {
                self.finish_redefine(redefine)?;
            }
        }

        Ok(())
    }
}
