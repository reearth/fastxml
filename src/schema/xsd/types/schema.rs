//! XSD schema and import/include types.

use std::collections::HashMap;

use super::attributes::XsdAttributeGroup;
use super::complex::XsdComplexType;
use super::elements::{XsdAttribute, XsdElement, XsdTypeDef};
use super::groups::XsdGroup;
use super::simple::XsdSimpleType;

/// A parsed XSD schema.
#[derive(Debug, Clone, Default)]
pub struct XsdSchema {
    /// Target namespace for this schema
    pub target_namespace: Option<String>,
    /// Element form default (qualified/unqualified)
    pub element_form_default: FormDefault,
    /// Attribute form default (qualified/unqualified)
    pub attribute_form_default: FormDefault,
    /// Schema version
    pub version: Option<String>,
    /// Import declarations
    pub imports: Vec<XsdImport>,
    /// Include declarations
    pub includes: Vec<XsdInclude>,
    /// Redefine declarations
    pub redefines: Vec<XsdRedefine>,
    /// Top-level element declarations
    pub elements: Vec<XsdElement>,
    /// Type definitions (simple and complex)
    pub types: Vec<XsdTypeDef>,
    /// Top-level attribute declarations
    pub attributes: Vec<XsdAttribute>,
    /// Attribute group definitions
    pub attribute_groups: Vec<XsdAttributeGroup>,
    /// Model group definitions
    pub groups: Vec<XsdGroup>,
    /// Namespace bindings (prefix -> URI)
    pub namespace_bindings: HashMap<String, String>,
}

impl XsdSchema {
    /// Creates a new empty schema.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the target namespace.
    pub fn with_target_namespace(mut self, ns: impl Into<String>) -> Self {
        self.target_namespace = Some(ns.into());
        self
    }
}

/// Form default for elements/attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormDefault {
    /// Elements/attributes are qualified by default
    Qualified,
    /// Elements/attributes are unqualified by default
    #[default]
    Unqualified,
}

/// Import declaration.
#[derive(Debug, Clone)]
pub struct XsdImport {
    /// Namespace being imported
    pub namespace: Option<String>,
    /// Schema location URL
    pub schema_location: Option<String>,
}

/// Include declaration.
#[derive(Debug, Clone)]
pub struct XsdInclude {
    /// Schema location URL
    pub schema_location: String,
}

/// Redefine declaration.
///
/// Allows modification of schema components from an included schema.
#[derive(Debug, Clone)]
pub struct XsdRedefine {
    /// Schema location URL
    pub schema_location: String,
    /// Redefined simple types
    pub simple_types: Vec<XsdSimpleType>,
    /// Redefined complex types
    pub complex_types: Vec<XsdComplexType>,
    /// Redefined model groups
    pub groups: Vec<XsdGroup>,
    /// Redefined attribute groups
    pub attribute_groups: Vec<XsdAttributeGroup>,
}

impl XsdRedefine {
    /// Creates a new redefine declaration.
    pub fn new(schema_location: impl Into<String>) -> Self {
        Self {
            schema_location: schema_location.into(),
            simple_types: Vec::new(),
            complex_types: Vec::new(),
            groups: Vec::new(),
            attribute_groups: Vec::new(),
        }
    }
}
