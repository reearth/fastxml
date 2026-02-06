//! XSD identity constraint definitions.

use super::qname::QName;

/// Type of identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XsdConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// Identity constraint definition (unique, key, keyref).
#[derive(Debug, Clone)]
pub struct XsdIdentityConstraint {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: XsdConstraintType,
    /// XPath selector expression
    pub selector: String,
    /// XPath field expressions (one or more for composite keys)
    pub fields: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<QName>,
}

impl XsdIdentityConstraint {
    /// Creates a new unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::Unique,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::Key,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new keyref constraint.
    pub fn keyref(name: impl Into<String>, selector: impl Into<String>, refer: QName) -> Self {
        Self {
            name: name.into(),
            constraint_type: XsdConstraintType::KeyRef,
            selector: selector.into(),
            fields: Vec::new(),
            refer: Some(refer),
        }
    }

    /// Adds a field expression.
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.fields.push(field.into());
        self
    }

    /// Adds multiple field expressions.
    pub fn with_fields(mut self, fields: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.fields.extend(fields.into_iter().map(Into::into));
        self
    }
}
