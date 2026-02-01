//! XSD Parser using XmlEventHandler.
//!
//! This parser converts XSD XML into AST types using a stack-based approach.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::event::{XmlEvent, XmlEventHandler};

use super::types::*;

/// XSD namespace URI.
pub const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

/// Parser state stack frame.
#[derive(Debug)]
enum StackFrame {
    /// Parsing xs:schema
    Schema,
    /// Parsing xs:element
    Element(XsdElement),
    /// Parsing xs:complexType
    ComplexType(XsdComplexType),
    /// Parsing xs:simpleType
    SimpleType(XsdSimpleType),
    /// Parsing xs:sequence
    Sequence(XsdSequence),
    /// Parsing xs:choice
    Choice(XsdChoice),
    /// Parsing xs:all
    All(XsdAll),
    /// Parsing xs:attribute
    Attribute(XsdAttribute),
    /// Parsing xs:attributeGroup
    AttributeGroup(XsdAttributeGroup),
    /// Parsing xs:group
    Group(XsdGroup),
    /// Parsing xs:restriction (simple type)
    SimpleRestriction(XsdSimpleRestriction),
    /// Parsing xs:extension (simple content)
    SimpleContentExtension(XsdSimpleContentExtension),
    /// Parsing xs:restriction (simple content)
    SimpleContentRestriction(XsdSimpleContentRestriction),
    /// Parsing xs:complexContent
    ComplexContent { mixed: bool },
    /// Parsing xs:simpleContent
    SimpleContent,
    /// Parsing xs:extension (complex content)
    ComplexContentExtension(XsdComplexContentExtension),
    /// Parsing xs:restriction (complex content)
    ComplexContentRestriction(XsdComplexContentRestriction),
    /// Parsing xs:list
    SimpleList(XsdSimpleList),
    /// Parsing xs:union
    SimpleUnion(XsdSimpleUnion),
    /// Parsing xs:any
    Any(XsdAny),
    /// Parsing xs:anyAttribute (ignored for now)
    AnyAttribute,
    /// Parsing annotation (skipped)
    Annotation,
    /// Parsing documentation (skipped)
    Documentation,
    /// Parsing appinfo (skipped)
    AppInfo,
    /// Parsing xs:unique identity constraint
    Unique(XsdIdentityConstraint),
    /// Parsing xs:key identity constraint
    Key(XsdIdentityConstraint),
    /// Parsing xs:keyref identity constraint
    KeyRef(XsdIdentityConstraint),
    /// Parsing xs:redefine
    Redefine(XsdRedefine),
}

/// XSD Parser that implements XmlEventHandler.
pub struct XsdParser {
    /// Stack of parsing states
    stack: Vec<StackFrame>,
    /// The schema being built
    schema: XsdSchema,
    /// Detected XSD namespace prefix (usually "xs" or "xsd")
    xsd_prefix: Option<String>,
    /// Current text content being collected
    current_text: String,
    /// Depth counter for skipping annotation content
    skip_depth: usize,
}

impl XsdParser {
    /// Creates a new XSD parser.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            schema: XsdSchema::new(),
            xsd_prefix: None,
            current_text: String::new(),
            skip_depth: 0,
        }
    }

    /// Consumes the parser and returns the parsed schema.
    pub fn into_schema(self) -> XsdSchema {
        self.schema
    }

    /// Returns a reference to the parsed schema.
    pub fn schema(&self) -> &XsdSchema {
        &self.schema
    }

    /// Checks if the element name matches an XSD element.
    fn is_xsd_element(&self, _name: &str, prefix: Option<&str>) -> bool {
        match (&self.xsd_prefix, prefix) {
            (Some(xsd_prefix), Some(p)) => p == xsd_prefix,
            (None, None) => true, // No prefix XSD (default namespace)
            (None, Some(_)) => false,
            (Some(_), None) => false,
        }
    }

    /// Gets the local name of an XSD element.
    fn xsd_local_name<'a>(&self, name: &'a str) -> &'a str {
        name
    }

    /// Parses attributes from a start element event.
    fn parse_attributes(attrs: &[(String, String)]) -> HashMap<String, String> {
        attrs.iter().cloned().collect()
    }

    /// Parses and validates minOccurs/maxOccurs from attributes.
    /// Returns (minOccurs, maxOccurs) or an error if invalid.
    fn parse_occurs(attrs: &HashMap<String, String>) -> Result<(Occurs, Occurs)> {
        let min = if let Some(min_str) = attrs.get("minOccurs") {
            Occurs::parse(min_str).map_err(|e| Error::Schema(e))?
        } else {
            Occurs::default()
        };

        let max = if let Some(max_str) = attrs.get("maxOccurs") {
            Occurs::parse(max_str).map_err(|e| Error::Schema(e))?
        } else {
            Occurs::default()
        };

        // Validate minOccurs <= maxOccurs
        match (&min, &max) {
            (Occurs::Count(min_val), Occurs::Count(max_val)) if min_val > max_val => {
                return Err(Error::Schema(format!(
                    "minOccurs ({}) cannot be greater than maxOccurs ({})",
                    min_val, max_val
                )));
            }
            _ => {}
        }

        Ok((min, max))
    }

    /// Handles a start element event.
    fn handle_start(
        &mut self,
        name: &str,
        prefix: Option<&str>,
        attrs: &[(String, String)],
        namespace_decls: &[crate::namespace::Namespace],
    ) -> Result<()> {
        // Check for XSD namespace binding
        if self.xsd_prefix.is_none() {
            for ns in namespace_decls {
                if ns.uri() == XSD_NAMESPACE {
                    if ns.prefix().is_empty() {
                        // Default namespace is XSD
                        self.xsd_prefix = None;
                    } else {
                        self.xsd_prefix = Some(ns.prefix().to_string());
                    }
                    break;
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
        let attr_map = Self::parse_attributes(attrs);

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

    fn handle_schema(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_element(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut elem = if let Some(ref_attr) = attrs.get("ref") {
            XsdElement::ref_(QName::parse(ref_attr))
        } else {
            XsdElement::new(attrs.get("name").cloned().unwrap_or_default())
        };

        if let Some(type_attr) = attrs.get("type") {
            elem.type_ref = Some(QName::parse(type_attr));
        }

        // Parse and validate minOccurs/maxOccurs
        let (min, max) = Self::parse_occurs(attrs)?;
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

    fn handle_complex_type(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_simple_type(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let st = if let Some(name) = attrs.get("name") {
            XsdSimpleType::new(name)
        } else {
            XsdSimpleType::anonymous()
        };

        self.stack.push(StackFrame::SimpleType(st));
        Ok(())
    }

    fn handle_sequence(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut seq = XsdSequence::default();
        let (min, max) = Self::parse_occurs(attrs)?;
        seq.min_occurs = min;
        seq.max_occurs = max;
        self.stack.push(StackFrame::Sequence(seq));
        Ok(())
    }

    fn handle_choice(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut choice = XsdChoice::default();
        let (min, max) = Self::parse_occurs(attrs)?;
        choice.min_occurs = min;
        choice.max_occurs = max;
        self.stack.push(StackFrame::Choice(choice));
        Ok(())
    }

    fn handle_all(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut all = XsdAll::default();
        // Parse minOccurs (maxOccurs for all is always 1 per XSD spec)
        if let Some(min_str) = attrs.get("minOccurs") {
            all.min_occurs = Occurs::parse(min_str).map_err(|e| Error::Schema(e))?;
        }
        all.max_occurs = Occurs::Count(1);
        self.stack.push(StackFrame::All(all));
        Ok(())
    }

    fn handle_attribute(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_attribute_group(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let ag = if let Some(ref_attr) = attrs.get("ref") {
            XsdAttributeGroup::ref_(QName::parse(ref_attr))
        } else {
            XsdAttributeGroup::new(attrs.get("name").cloned().unwrap_or_default())
        };
        self.stack.push(StackFrame::AttributeGroup(ag));
        Ok(())
    }

    fn handle_group(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut grp = if let Some(ref_attr) = attrs.get("ref") {
            XsdGroup::ref_(QName::parse(ref_attr))
        } else {
            XsdGroup::new(attrs.get("name").cloned().unwrap_or_default())
        };

        let (min, max) = Self::parse_occurs(attrs)?;
        grp.min_occurs = min;
        grp.max_occurs = max;

        self.stack.push(StackFrame::Group(grp));
        Ok(())
    }

    fn handle_restriction(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_extension(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_complex_content(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mixed = attrs.get("mixed").is_some_and(|v| v == "true");
        self.stack.push(StackFrame::ComplexContent { mixed });
        Ok(())
    }

    fn handle_simple_content(&mut self, _attrs: &HashMap<String, String>) -> Result<()> {
        self.stack.push(StackFrame::SimpleContent);
        Ok(())
    }

    fn handle_import(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let import = XsdImport {
            namespace: attrs.get("namespace").cloned(),
            schema_location: attrs.get("schemaLocation").cloned(),
        };
        self.schema.imports.push(import);
        Ok(())
    }

    fn handle_include(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        if let Some(loc) = attrs.get("schemaLocation") {
            let include = XsdInclude {
                schema_location: loc.clone(),
            };
            self.schema.includes.push(include);
        }
        Ok(())
    }

    fn handle_redefine(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let schema_location = attrs.get("schemaLocation").cloned().unwrap_or_default();
        let redefine = XsdRedefine::new(schema_location);
        self.stack.push(StackFrame::Redefine(redefine));
        Ok(())
    }

    fn handle_unique(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        // Selector will be set when we encounter the selector element
        let constraint = XsdIdentityConstraint::unique(name, "");
        self.stack.push(StackFrame::Unique(constraint));
        Ok(())
    }

    fn handle_key(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        let constraint = XsdIdentityConstraint::key(name, "");
        self.stack.push(StackFrame::Key(constraint));
        Ok(())
    }

    fn handle_keyref(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let name = attrs.get("name").cloned().unwrap_or_default();
        let refer = attrs
            .get("refer")
            .map(|s| QName::parse(s))
            .unwrap_or_else(|| QName::new(""));
        let constraint = XsdIdentityConstraint::keyref(name, "", refer);
        self.stack.push(StackFrame::KeyRef(constraint));
        Ok(())
    }

    fn handle_selector(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_field(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_list(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let list = XsdSimpleList {
            item_type: attrs.get("itemType").map(|s| QName::parse(s)),
            inline_type: None,
        };
        self.stack.push(StackFrame::SimpleList(list));
        Ok(())
    }

    fn handle_union(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
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

    fn handle_any(&mut self, attrs: &HashMap<String, String>) -> Result<()> {
        let mut any = XsdAny::default();

        let (min, max) = Self::parse_occurs(attrs)?;
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

    /// Parses a non-negative integer facet value with validation.
    fn parse_facet_length(name: &str, value: &str) -> Result<u32> {
        // Check for negative values
        if value.starts_with('-') {
            return Err(Error::Schema(format!(
                "invalid {} value '{}': must be non-negative",
                name, value
            )));
        }
        value.parse::<u32>().map_err(|_| {
            Error::Schema(format!(
                "invalid {} value '{}': must be a non-negative integer",
                name, value
            ))
        })
    }

    /// Validates that existing facets in a restriction are consistent with a new facet.
    fn validate_facet_consistency(facets: &[XsdFacet], new_facet: &XsdFacet) -> Result<()> {
        match new_facet {
            XsdFacet::MinLength(min_len) => {
                // Check if maxLength exists and is less than minLength
                for f in facets {
                    if let XsdFacet::MaxLength(max_len) = f {
                        if min_len > max_len {
                            return Err(Error::Schema(format!(
                                "minLength ({}) cannot be greater than maxLength ({})",
                                min_len, max_len
                            )));
                        }
                    }
                }
            }
            XsdFacet::MaxLength(max_len) => {
                // Check if minLength exists and is greater than maxLength
                for f in facets {
                    if let XsdFacet::MinLength(min_len) = f {
                        if min_len > max_len {
                            return Err(Error::Schema(format!(
                                "minLength ({}) cannot be greater than maxLength ({})",
                                min_len, max_len
                            )));
                        }
                    }
                }
            }
            XsdFacet::FractionDigits(frac) => {
                // fractionDigits must be <= totalDigits
                for f in facets {
                    if let XsdFacet::TotalDigits(total) = f {
                        if frac > total {
                            return Err(Error::Schema(format!(
                                "fractionDigits ({}) cannot be greater than totalDigits ({})",
                                frac, total
                            )));
                        }
                    }
                }
            }
            XsdFacet::TotalDigits(total) => {
                // totalDigits must be >= fractionDigits
                for f in facets {
                    if let XsdFacet::FractionDigits(frac) = f {
                        if frac > total {
                            return Err(Error::Schema(format!(
                                "fractionDigits ({}) cannot be greater than totalDigits ({})",
                                frac, total
                            )));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_facet(&mut self, name: &str, attrs: &HashMap<String, String>) -> Result<()> {
        let value = attrs.get("value").cloned().unwrap_or_default();

        let facet = match name {
            "enumeration" => XsdFacet::Enumeration(value),
            "pattern" => XsdFacet::Pattern(value),
            "minLength" => XsdFacet::MinLength(Self::parse_facet_length("minLength", &value)?),
            "maxLength" => XsdFacet::MaxLength(Self::parse_facet_length("maxLength", &value)?),
            "length" => XsdFacet::Length(Self::parse_facet_length("length", &value)?),
            "minInclusive" => XsdFacet::MinInclusive(value),
            "maxInclusive" => XsdFacet::MaxInclusive(value),
            "minExclusive" => XsdFacet::MinExclusive(value),
            "maxExclusive" => XsdFacet::MaxExclusive(value),
            "totalDigits" => XsdFacet::TotalDigits(Self::parse_facet_length("totalDigits", &value)?),
            "fractionDigits" => XsdFacet::FractionDigits(Self::parse_facet_length("fractionDigits", &value)?),
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
    fn handle_end(&mut self, name: &str, prefix: Option<&str>) -> Result<()> {
        // Handle annotation skipping
        if self.skip_depth > 0 {
            self.skip_depth -= 1;
            if self.skip_depth == 0 {
                // Pop the annotation frame
                self.stack.pop();
            }
            return Ok(());
        }

        if !self.is_xsd_element(name, prefix) {
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

    fn finish_element(&mut self, elem: XsdElement) -> Result<()> {
        // Find parent context
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.elements.push(elem);
                }
                StackFrame::Sequence(seq) => {
                    seq.particles.push(XsdParticleItem::Element(elem));
                }
                StackFrame::Choice(choice) => {
                    choice.particles.push(XsdParticleItem::Element(elem));
                }
                StackFrame::All(all) => {
                    all.elements.push(elem);
                }
                StackFrame::ComplexContentExtension(ext) => {
                    // Element in extension without explicit particle
                    if ext.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        ext.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                StackFrame::ComplexContentRestriction(r) => {
                    // Element in restriction without explicit particle
                    if r.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        r.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                StackFrame::Group(grp) => {
                    // Element directly in group (unusual but possible)
                    if grp.particle.is_none() {
                        let mut seq = XsdSequence::default();
                        seq.particles.push(XsdParticleItem::Element(elem));
                        grp.particle = Some(XsdParticle::Sequence(seq));
                    }
                }
                _ => {}
            }
        } else {
            // Top-level element
            self.schema.elements.push(elem);
        }
        Ok(())
    }

    fn finish_complex_type(&mut self, ct: XsdComplexType) -> Result<()> {
        let type_def = XsdTypeDef::Complex(ct);

        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.types.push(type_def);
                }
                StackFrame::Element(elem) => {
                    elem.inline_type = Some(Box::new(type_def));
                }
                _ => {}
            }
        } else {
            self.schema.types.push(type_def);
        }
        Ok(())
    }

    fn finish_simple_type(&mut self, st: XsdSimpleType) -> Result<()> {
        let type_def = XsdTypeDef::Simple(st.clone());

        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.types.push(type_def);
                }
                StackFrame::Element(elem) => {
                    elem.inline_type = Some(Box::new(type_def));
                }
                StackFrame::Attribute(attr) => {
                    attr.inline_type = Some(st);
                }
                StackFrame::SimpleRestriction(r) => {
                    r.inline_base = Some(Box::new(st));
                }
                StackFrame::SimpleList(list) => {
                    list.inline_type = Some(Box::new(st));
                }
                StackFrame::SimpleUnion(union) => {
                    union.inline_types.push(st);
                }
                _ => {}
            }
        } else {
            self.schema.types.push(type_def);
        }
        Ok(())
    }

    fn finish_sequence(&mut self, seq: XsdSequence) -> Result<()> {
        let particle = XsdParticle::Sequence(seq);

        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::ComplexType(ct) => {
                    ct.content = XsdComplexContent::Particle(particle);
                }
                StackFrame::Sequence(parent_seq) => {
                    parent_seq
                        .particles
                        .push(XsdParticleItem::Sequence(match particle {
                            XsdParticle::Sequence(s) => s,
                            _ => unreachable!(),
                        }));
                }
                StackFrame::Choice(choice) => {
                    choice
                        .particles
                        .push(XsdParticleItem::Sequence(match particle {
                            XsdParticle::Sequence(s) => s,
                            _ => unreachable!(),
                        }));
                }
                StackFrame::ComplexContentExtension(ext) => {
                    ext.particle = Some(particle);
                }
                StackFrame::ComplexContentRestriction(r) => {
                    r.particle = Some(particle);
                }
                StackFrame::Group(grp) => {
                    grp.particle = Some(particle);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn finish_choice(&mut self, choice: XsdChoice) -> Result<()> {
        let particle = XsdParticle::Choice(choice);

        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::ComplexType(ct) => {
                    ct.content = XsdComplexContent::Particle(particle);
                }
                StackFrame::Sequence(seq) => {
                    seq.particles.push(XsdParticleItem::Choice(match particle {
                        XsdParticle::Choice(c) => c,
                        _ => unreachable!(),
                    }));
                }
                StackFrame::Choice(parent_choice) => {
                    parent_choice
                        .particles
                        .push(XsdParticleItem::Choice(match particle {
                            XsdParticle::Choice(c) => c,
                            _ => unreachable!(),
                        }));
                }
                StackFrame::ComplexContentExtension(ext) => {
                    ext.particle = Some(particle);
                }
                StackFrame::ComplexContentRestriction(r) => {
                    r.particle = Some(particle);
                }
                StackFrame::Group(grp) => {
                    grp.particle = Some(particle);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn finish_all(&mut self, all: XsdAll) -> Result<()> {
        let particle = XsdParticle::All(all);

        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::ComplexType(ct) => {
                    ct.content = XsdComplexContent::Particle(particle);
                }
                StackFrame::ComplexContentExtension(ext) => {
                    ext.particle = Some(particle);
                }
                StackFrame::ComplexContentRestriction(r) => {
                    r.particle = Some(particle);
                }
                StackFrame::Group(grp) => {
                    grp.particle = Some(particle);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn finish_attribute(&mut self, attr: XsdAttribute) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Schema => {
                    self.schema.attributes.push(attr);
                }
                StackFrame::ComplexType(ct) => {
                    ct.attributes.push(attr);
                }
                StackFrame::AttributeGroup(ag) => {
                    ag.attributes.push(attr);
                }
                StackFrame::SimpleContentExtension(ext) => {
                    ext.attributes.push(attr);
                }
                StackFrame::SimpleContentRestriction(r) => {
                    r.attributes.push(attr);
                }
                StackFrame::ComplexContentExtension(ext) => {
                    ext.attributes.push(attr);
                }
                StackFrame::ComplexContentRestriction(r) => {
                    r.attributes.push(attr);
                }
                _ => {}
            }
        } else {
            self.schema.attributes.push(attr);
        }
        Ok(())
    }

    fn finish_attribute_group(&mut self, ag: XsdAttributeGroup) -> Result<()> {
        // If it's a reference, add it to the parent's attribute groups
        if ag.ref_.is_some() {
            if let Some(ref_qname) = ag.ref_.clone() {
                if let Some(parent) = self.stack.last_mut() {
                    match parent {
                        StackFrame::ComplexType(ct) => {
                            ct.attribute_groups.push(ref_qname);
                        }
                        StackFrame::AttributeGroup(parent_ag) => {
                            parent_ag.attribute_groups.push(ref_qname);
                        }
                        StackFrame::SimpleContentExtension(ext) => {
                            ext.attribute_groups.push(ref_qname);
                        }
                        StackFrame::SimpleContentRestriction(r) => {
                            r.attribute_groups.push(ref_qname);
                        }
                        StackFrame::ComplexContentExtension(ext) => {
                            ext.attribute_groups.push(ref_qname);
                        }
                        StackFrame::ComplexContentRestriction(r) => {
                            r.attribute_groups.push(ref_qname);
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // It's a definition
            if let Some(parent) = self.stack.last_mut() {
                if matches!(parent, StackFrame::Schema) {
                    self.schema.attribute_groups.push(ag);
                }
            } else {
                self.schema.attribute_groups.push(ag);
            }
        }
        Ok(())
    }

    fn finish_group(&mut self, grp: XsdGroup) -> Result<()> {
        // If it's a reference, add it to the parent particle
        if grp.ref_.is_some() {
            if let Some(ref_qname) = grp.ref_.clone() {
                if let Some(parent) = self.stack.last_mut() {
                    match parent {
                        StackFrame::Sequence(seq) => {
                            seq.particles.push(XsdParticleItem::GroupRef(ref_qname));
                        }
                        StackFrame::Choice(choice) => {
                            choice.particles.push(XsdParticleItem::GroupRef(ref_qname));
                        }
                        StackFrame::ComplexType(ct) => {
                            ct.content =
                                XsdComplexContent::Particle(XsdParticle::GroupRef(ref_qname));
                        }
                        StackFrame::ComplexContentExtension(ext) => {
                            ext.particle = Some(XsdParticle::GroupRef(ref_qname));
                        }
                        StackFrame::ComplexContentRestriction(r) => {
                            r.particle = Some(XsdParticle::GroupRef(ref_qname));
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // It's a definition
            if let Some(parent) = self.stack.last_mut() {
                if matches!(parent, StackFrame::Schema) {
                    self.schema.groups.push(grp);
                }
            } else {
                self.schema.groups.push(grp);
            }
        }
        Ok(())
    }

    fn finish_simple_restriction(&mut self, r: XsdSimpleRestriction) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            if let StackFrame::SimpleType(st) = parent {
                st.content = XsdSimpleTypeContent::Restriction(r);
            }
        }
        Ok(())
    }

    fn finish_simple_content_extension(&mut self, ext: XsdSimpleContentExtension) -> Result<()> {
        // Find the parent complexType or simpleContent
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::ComplexType(ct) = frame {
                ct.content = XsdComplexContent::SimpleContent(XsdSimpleContentDef {
                    derivation: XsdSimpleContentDerivation::Extension(ext),
                });
                break;
            }
        }
        Ok(())
    }

    fn finish_simple_content_restriction(&mut self, r: XsdSimpleContentRestriction) -> Result<()> {
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::ComplexType(ct) = frame {
                ct.content = XsdComplexContent::SimpleContent(XsdSimpleContentDef {
                    derivation: XsdSimpleContentDerivation::Restriction(r),
                });
                break;
            }
        }
        Ok(())
    }

    fn finish_complex_content(&mut self, mixed: bool) -> Result<()> {
        // Set mixed on parent complexType if specified
        if mixed {
            for frame in self.stack.iter_mut().rev() {
                if let StackFrame::ComplexType(ct) = frame {
                    ct.mixed = true;
                    break;
                }
            }
        }
        Ok(())
    }

    fn finish_complex_content_extension(&mut self, ext: XsdComplexContentExtension) -> Result<()> {
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::ComplexType(ct) = frame {
                ct.content = XsdComplexContent::ComplexContent(XsdComplexContentDef {
                    mixed: ct.mixed,
                    derivation: XsdComplexContentDerivation::Extension(ext),
                });
                break;
            }
        }
        Ok(())
    }

    fn finish_complex_content_restriction(
        &mut self,
        r: XsdComplexContentRestriction,
    ) -> Result<()> {
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::ComplexType(ct) = frame {
                ct.content = XsdComplexContent::ComplexContent(XsdComplexContentDef {
                    mixed: ct.mixed,
                    derivation: XsdComplexContentDerivation::Restriction(r),
                });
                break;
            }
        }
        Ok(())
    }

    fn finish_simple_list(&mut self, list: XsdSimpleList) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            if let StackFrame::SimpleType(st) = parent {
                st.content = XsdSimpleTypeContent::List(list);
            }
        }
        Ok(())
    }

    fn finish_simple_union(&mut self, union: XsdSimpleUnion) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            if let StackFrame::SimpleType(st) = parent {
                st.content = XsdSimpleTypeContent::Union(union);
            }
        }
        Ok(())
    }

    fn finish_any(&mut self, any: XsdAny) -> Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            match parent {
                StackFrame::Sequence(seq) => {
                    seq.particles.push(XsdParticleItem::Any(any));
                }
                StackFrame::Choice(choice) => {
                    choice.particles.push(XsdParticleItem::Any(any));
                }
                StackFrame::ComplexType(ct) => {
                    ct.content = XsdComplexContent::Particle(XsdParticle::Any(any));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn finish_identity_constraint(&mut self, constraint: XsdIdentityConstraint) -> Result<()> {
        // Identity constraints are always children of element declarations
        for frame in self.stack.iter_mut().rev() {
            if let StackFrame::Element(elem) = frame {
                elem.identity_constraints.push(constraint);
                return Ok(());
            }
        }
        Ok(())
    }

    fn finish_redefine(&mut self, redefine: XsdRedefine) -> Result<()> {
        // Redefine is a top-level schema component
        self.schema.redefines.push(redefine);
        Ok(())
    }
}

impl Default for XsdParser {
    fn default() -> Self {
        Self::new()
    }
}

impl XmlEventHandler for XsdParser {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                attributes,
                namespace_decls,
                ..
            } => {
                self.handle_start(name, prefix.as_deref(), attributes, namespace_decls)?;
            }
            XmlEvent::EndElement { name, prefix } => {
                self.handle_end(name, prefix.as_deref())?;
            }
            XmlEvent::Text(text) => {
                self.current_text.push_str(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if !self.stack.is_empty() {
            return Err(Error::XsdParse(format!(
                "Unexpected end of schema, stack not empty: {} frames remaining",
                self.stack.len()
            )));
        }
        Ok(())
    }
}

/// Parses XSD content into an AST.
pub fn parse_xsd_ast(content: &[u8]) -> Result<XsdSchema> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_reader(content);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;

    let mut xsd_parser = XsdParser::new();
    let mut buf = Vec::with_capacity(8 * 1024);

    loop {
        let event_result = reader.read_event_into(&mut buf);
        let position = reader.buffer_position();

        match event_result {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let is_empty = matches!(event_result, Ok(Event::Empty(_)));
                let xml_event = convert_start_event(e, position)?;
                xsd_parser.handle(&xml_event)?;

                if is_empty {
                    if let XmlEvent::StartElement { name, prefix, .. } = &xml_event {
                        let end_event = XmlEvent::EndElement {
                            name: name.clone(),
                            prefix: prefix.clone(),
                        };
                        xsd_parser.handle(&end_event)?;
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name_bytes = e.name().as_ref().to_vec();
                let full_name = std::str::from_utf8(&name_bytes)?;
                let (prefix, name) = crate::namespace::split_qname(full_name);
                let event = XmlEvent::EndElement {
                    name: name.to_string(),
                    prefix: prefix.map(String::from),
                };
                xsd_parser.handle(&event)?;
            }
            Ok(Event::Text(ref e)) => {
                let text = e
                    .unescape()
                    .map_err(|e| Error::Parse(format!("text decode error: {}", e)))?;
                if !text.is_empty() {
                    let event = XmlEvent::Text(text.into_owned());
                    xsd_parser.handle(&event)?;
                }
            }
            Ok(Event::Eof) => {
                xsd_parser.handle(&XmlEvent::Eof)?;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                return Err(Error::XsdParse(format!(
                    "parse error at position {}: {}",
                    position, e
                )));
            }
        }
        buf.clear();
    }

    xsd_parser.finish()?;
    Ok(xsd_parser.into_schema())
}

fn convert_start_event(e: &quick_xml::events::BytesStart<'_>, position: u64) -> Result<XmlEvent> {
    let name_bytes = e.name().as_ref().to_vec();
    let full_name = std::str::from_utf8(&name_bytes)?;
    let (prefix, name) = crate::namespace::split_qname(full_name);

    let mut namespace_decls = Vec::new();
    let mut attributes = Vec::new();

    for attr_result in e.attributes() {
        let attr = attr_result?;
        let key = std::str::from_utf8(attr.key.as_ref())?;
        let value = attr
            .unescape_value()
            .map_err(|e| Error::Parse(format!("attribute value decode error: {}", e)))?;

        if key == "xmlns" {
            namespace_decls.push(crate::namespace::Namespace::default_ns(value.as_ref()));
        } else if let Some(ns_prefix) = key.strip_prefix("xmlns:") {
            namespace_decls.push(crate::namespace::Namespace::new(ns_prefix, value.as_ref()));
        } else {
            attributes.push((key.to_string(), value.to_string()));
        }
    }

    let line = Some(position as usize);

    Ok(XmlEvent::StartElement {
        name: name.to_string(),
        prefix: prefix.map(String::from),
        namespace: None,
        attributes,
        namespace_decls,
        line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_schema() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                   targetNamespace="http://example.com/test"
                   elementFormDefault="qualified">
            <xs:element name="root" type="xs:string"/>
        </xs:schema>"#;

        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        assert_eq!(
            schema.target_namespace,
            Some("http://example.com/test".to_string())
        );
        assert_eq!(schema.element_form_default, FormDefault::Qualified);
        assert_eq!(schema.elements.len(), 1);
        assert_eq!(schema.elements[0].name, "root");
    }

    #[test]
    fn test_parse_complex_type() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="PersonType">
                <xs:sequence>
                    <xs:element name="name" type="xs:string"/>
                    <xs:element name="age" type="xs:integer" minOccurs="0"/>
                </xs:sequence>
                <xs:attribute name="id" type="xs:ID" use="required"/>
            </xs:complexType>
        </xs:schema>"#;

        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        assert_eq!(schema.types.len(), 1);

        if let XsdTypeDef::Complex(ct) = &schema.types[0] {
            assert_eq!(ct.name, Some("PersonType".to_string()));
            assert_eq!(ct.attributes.len(), 1);
            assert_eq!(ct.attributes[0].name, Some("id".to_string()));
            assert_eq!(ct.attributes[0].use_, AttributeUse::Required);

            if let XsdComplexContent::Particle(XsdParticle::Sequence(seq)) = &ct.content {
                assert_eq!(seq.particles.len(), 2);
            } else {
                panic!("Expected sequence");
            }
        } else {
            panic!("Expected complex type");
        }
    }

    #[test]
    fn test_parse_simple_type_restriction() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:simpleType name="StatusType">
                <xs:restriction base="xs:string">
                    <xs:enumeration value="active"/>
                    <xs:enumeration value="inactive"/>
                    <xs:enumeration value="pending"/>
                </xs:restriction>
            </xs:simpleType>
        </xs:schema>"#;

        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        assert_eq!(schema.types.len(), 1);

        if let XsdTypeDef::Simple(st) = &schema.types[0] {
            assert_eq!(st.name, Some("StatusType".to_string()));
            if let XsdSimpleTypeContent::Restriction(r) = &st.content {
                assert_eq!(r.facets.len(), 3);
                assert!(matches!(&r.facets[0], XsdFacet::Enumeration(v) if v == "active"));
            } else {
                panic!("Expected restriction");
            }
        } else {
            panic!("Expected simple type");
        }
    }

    #[test]
    fn test_parse_import() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:import namespace="http://www.opengis.net/gml/3.2"
                       schemaLocation="http://schemas.opengis.net/gml/3.2.1/gml.xsd"/>
        </xs:schema>"#;

        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        assert_eq!(schema.imports.len(), 1);
        assert_eq!(
            schema.imports[0].namespace,
            Some("http://www.opengis.net/gml/3.2".to_string())
        );
        assert_eq!(
            schema.imports[0].schema_location,
            Some("http://schemas.opengis.net/gml/3.2.1/gml.xsd".to_string())
        );
    }

    #[test]
    fn test_parse_extension() {
        let xsd = r#"<?xml version="1.0" encoding="UTF-8"?>
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="ExtendedType">
                <xs:complexContent>
                    <xs:extension base="BaseType">
                        <xs:sequence>
                            <xs:element name="extra" type="xs:string"/>
                        </xs:sequence>
                    </xs:extension>
                </xs:complexContent>
            </xs:complexType>
        </xs:schema>"#;

        let schema = parse_xsd_ast(xsd.as_bytes()).unwrap();
        assert_eq!(schema.types.len(), 1);

        if let XsdTypeDef::Complex(ct) = &schema.types[0] {
            if let XsdComplexContent::ComplexContent(cc) = &ct.content {
                if let XsdComplexContentDerivation::Extension(ext) = &cc.derivation {
                    assert_eq!(ext.base.local, "BaseType");
                    assert!(ext.particle.is_some());
                } else {
                    panic!("Expected extension");
                }
            } else {
                panic!("Expected complex content");
            }
        } else {
            panic!("Expected complex type");
        }
    }
}
