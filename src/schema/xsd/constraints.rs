//! XSD Identity Constraints.
//!
//! This module implements validation for XSD identity constraints:
//!
//! ## Constraint Types
//! - `unique` - values must be unique within scope
//! - `key` - values must be unique and non-null
//! - `keyref` - values must reference an existing key
//!
//! ## Components
//! - `selector` - XPath expression selecting the scope
//! - `field` - XPath expression selecting the key value(s)

use std::collections::{HashMap, HashSet};

/// Type of identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintType {
    /// Values must be unique (null allowed)
    Unique,
    /// Values must be unique and non-null
    Key,
    /// Values must reference an existing key
    KeyRef,
}

/// Identity constraint definition from XSD.
#[derive(Debug, Clone)]
pub struct IdentityConstraint {
    /// Constraint name
    pub name: String,
    /// Type of constraint
    pub constraint_type: ConstraintType,
    /// XPath selector expression
    pub selector: String,
    /// XPath field expressions (one or more for composite keys)
    pub fields: Vec<String>,
    /// For keyref: the key being referenced
    pub refer: Option<String>,
}

impl IdentityConstraint {
    /// Creates a new unique constraint.
    pub fn unique(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: ConstraintType::Unique,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new key constraint.
    pub fn key(name: impl Into<String>, selector: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            constraint_type: ConstraintType::Key,
            selector: selector.into(),
            fields: Vec::new(),
            refer: None,
        }
    }

    /// Creates a new keyref constraint.
    pub fn keyref(
        name: impl Into<String>,
        selector: impl Into<String>,
        refer: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            constraint_type: ConstraintType::KeyRef,
            selector: selector.into(),
            fields: Vec::new(),
            refer: Some(refer.into()),
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

/// A key value (possibly composite).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyValue {
    /// Field values making up the key
    pub values: Vec<String>,
}

impl KeyValue {
    /// Creates a new key value from field values.
    pub fn new(values: Vec<String>) -> Self {
        Self { values }
    }

    /// Creates a single-field key value.
    pub fn single(value: impl Into<String>) -> Self {
        Self {
            values: vec![value.into()],
        }
    }

    /// Returns true if any field is null/empty.
    pub fn has_null(&self) -> bool {
        self.values.iter().any(|v| v.is_empty())
    }

    /// Returns true if all fields are present.
    pub fn is_complete(&self, expected_fields: usize) -> bool {
        self.values.len() == expected_fields && !self.has_null()
    }
}

/// Error types for constraint validation.
#[derive(Debug, Clone)]
pub enum ConstraintError {
    /// Duplicate key value found
    DuplicateKey {
        /// Name of the constraint that found the duplicate
        constraint: String,
        /// The duplicate key value
        value: KeyValue,
    },
    /// Null value in key (not allowed for key, allowed for unique)
    NullKeyValue {
        /// Name of the constraint with null value
        constraint: String,
        /// Index of the field that is null
        field_index: usize,
    },
    /// Key reference not found
    KeyRefNotFound {
        /// Name of the keyref constraint
        constraint: String,
        /// Name of the referenced key constraint
        refer: String,
        /// The value that was not found
        value: KeyValue,
    },
    /// Selector expression error
    SelectorError {
        /// Name of the constraint with selector error
        constraint: String,
        /// Error message
        message: String,
    },
    /// Field expression error
    FieldError {
        /// Name of the constraint with field error
        constraint: String,
        /// Index of the field with error
        field_index: usize,
        /// Error message
        message: String,
    },
}

impl std::fmt::Display for ConstraintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstraintError::DuplicateKey { constraint, value } => {
                write!(
                    f,
                    "duplicate value {:?} in constraint '{}'",
                    value.values, constraint
                )
            }
            ConstraintError::NullKeyValue {
                constraint,
                field_index,
            } => {
                write!(
                    f,
                    "null value in key field {} of constraint '{}'",
                    field_index, constraint
                )
            }
            ConstraintError::KeyRefNotFound {
                constraint,
                refer,
                value,
            } => {
                write!(
                    f,
                    "keyref '{}' value {:?} not found in key '{}'",
                    constraint, value.values, refer
                )
            }
            ConstraintError::SelectorError {
                constraint,
                message,
            } => {
                write!(
                    f,
                    "selector error in constraint '{}': {}",
                    constraint, message
                )
            }
            ConstraintError::FieldError {
                constraint,
                field_index,
                message,
            } => {
                write!(
                    f,
                    "field {} error in constraint '{}': {}",
                    field_index, constraint, message
                )
            }
        }
    }
}

impl std::error::Error for ConstraintError {}

/// Collected key values for a constraint.
#[derive(Debug, Clone, Default)]
pub struct KeyValueSet {
    /// Unique key values
    values: HashSet<KeyValue>,
    /// Constraint name
    constraint_name: String,
    /// Expected number of fields (for composite key validation)
    #[allow(dead_code)]
    field_count: usize,
}

impl KeyValueSet {
    /// Creates a new key value set.
    pub fn new(constraint_name: impl Into<String>, field_count: usize) -> Self {
        Self {
            values: HashSet::new(),
            constraint_name: constraint_name.into(),
            field_count,
        }
    }

    /// Adds a key value, checking for duplicates.
    ///
    /// Returns error if duplicate (for unique/key constraints).
    pub fn add(&mut self, value: KeyValue) -> Result<(), ConstraintError> {
        if !self.values.insert(value.clone()) {
            return Err(ConstraintError::DuplicateKey {
                constraint: self.constraint_name.clone(),
                value,
            });
        }
        Ok(())
    }

    /// Checks if a value exists in the set.
    pub fn contains(&self, value: &KeyValue) -> bool {
        self.values.contains(value)
    }

    /// Returns all values in the set.
    pub fn values(&self) -> impl Iterator<Item = &KeyValue> {
        self.values.iter()
    }

    /// Returns the number of values.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true if empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// Constraint validator that tracks key values during validation.
#[derive(Debug, Default)]
pub struct ConstraintValidator {
    /// Collected key values indexed by constraint name
    key_values: HashMap<String, KeyValueSet>,
    /// Pending keyref validations (checked at end)
    pending_keyrefs: Vec<PendingKeyRef>,
}

#[derive(Debug)]
struct PendingKeyRef {
    constraint_name: String,
    refer: String,
    value: KeyValue,
}

impl ConstraintValidator {
    /// Creates a new constraint validator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a key constraint and initializes its value set.
    pub fn register_key(&mut self, name: &str, field_count: usize) {
        self.key_values
            .insert(name.to_string(), KeyValueSet::new(name, field_count));
    }

    /// Adds a key value from a unique or key constraint.
    pub fn add_key_value(
        &mut self,
        constraint: &IdentityConstraint,
        value: KeyValue,
    ) -> Result<(), ConstraintError> {
        // For key constraints, null values are not allowed
        if constraint.constraint_type == ConstraintType::Key && value.has_null() {
            for (idx, v) in value.values.iter().enumerate() {
                if v.is_empty() {
                    return Err(ConstraintError::NullKeyValue {
                        constraint: constraint.name.clone(),
                        field_index: idx,
                    });
                }
            }
        }

        // Get or create the key value set
        let set = self
            .key_values
            .entry(constraint.name.clone())
            .or_insert_with(|| KeyValueSet::new(&constraint.name, constraint.fields.len()));

        // For unique constraints, null values don't participate in uniqueness check
        if constraint.constraint_type == ConstraintType::Unique && value.has_null() {
            return Ok(());
        }

        set.add(value)
    }

    /// Records a key/unique value into the shared, name-keyed table purely so
    /// later keyrefs can resolve against it, without performing a uniqueness
    /// check. Uniqueness is scope-local and must be enforced by the caller per
    /// scoping-element instance; the shared table deliberately unions values
    /// across all instances of a constraint (mirroring the DOM engine, which
    /// checks `seen` per scope but merges into name-keyed tables for keyref).
    pub fn record_key_value(&mut self, constraint: &IdentityConstraint, value: KeyValue) {
        if constraint.constraint_type == ConstraintType::Unique && value.has_null() {
            return;
        }
        let set = self
            .key_values
            .entry(constraint.name.clone())
            .or_insert_with(|| KeyValueSet::new(&constraint.name, constraint.fields.len()));
        // Ignore the duplicate result: the table is a union for keyref lookup.
        let _ = set.add(value);
    }

    /// Adds a keyref value to be validated at the end.
    pub fn add_keyref_value(&mut self, constraint: &IdentityConstraint, value: KeyValue) {
        if let Some(refer) = &constraint.refer {
            // Null keyref values don't need to match
            if value.has_null() {
                return;
            }

            self.pending_keyrefs.push(PendingKeyRef {
                constraint_name: constraint.name.clone(),
                refer: refer.clone(),
                value,
            });
        }
    }

    /// Validates all pending keyrefs against collected keys.
    pub fn validate_keyrefs(&self) -> Result<(), Vec<ConstraintError>> {
        let mut errors = Vec::new();

        for keyref in &self.pending_keyrefs {
            if let Some(key_set) = self.key_values.get(&keyref.refer) {
                if !key_set.contains(&keyref.value) {
                    errors.push(ConstraintError::KeyRefNotFound {
                        constraint: keyref.constraint_name.clone(),
                        refer: keyref.refer.clone(),
                        value: keyref.value.clone(),
                    });
                }
            } else {
                // Referenced key doesn't exist
                errors.push(ConstraintError::KeyRefNotFound {
                    constraint: keyref.constraint_name.clone(),
                    refer: keyref.refer.clone(),
                    value: keyref.value.clone(),
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Resets the validator state.
    pub fn reset(&mut self) {
        self.key_values.clear();
        self.pending_keyrefs.clear();
    }

    /// Gets the key value set for a constraint.
    pub fn get_key_values(&self, name: &str) -> Option<&KeyValueSet> {
        self.key_values.get(name)
    }
}

/// Compiled identity constraint ready for validation.
#[derive(Debug, Clone)]
pub struct CompiledConstraint {
    /// Constraint name
    pub name: String,
    /// Constraint type
    pub constraint_type: ConstraintType,
    /// Compiled selector XPath (as string for now)
    pub selector_xpath: String,
    /// Compiled field XPaths
    pub field_xpaths: Vec<String>,
    /// Referenced key name (for keyref)
    pub refer: Option<String>,
}

impl CompiledConstraint {
    /// Creates a new compiled constraint.
    pub fn new(constraint: &IdentityConstraint) -> Self {
        Self {
            name: constraint.name.clone(),
            constraint_type: constraint.constraint_type,
            selector_xpath: constraint.selector.clone(),
            field_xpaths: constraint.fields.clone(),
            refer: constraint.refer.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_constraint() {
        let constraint = IdentityConstraint::unique("id-unique", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Add first value
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single("1"))
                .is_ok()
        );

        // Add different value
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single("2"))
                .is_ok()
        );

        // Add duplicate - should fail
        let result = validator.add_key_value(&constraint, KeyValue::single("1"));
        assert!(matches!(result, Err(ConstraintError::DuplicateKey { .. })));
    }

    #[test]
    fn test_key_constraint_no_nulls() {
        let constraint = IdentityConstraint::key("item-key", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Null value should fail for key
        let result = validator.add_key_value(&constraint, KeyValue::single(""));
        assert!(matches!(result, Err(ConstraintError::NullKeyValue { .. })));
    }

    #[test]
    fn test_unique_constraint_allows_nulls() {
        let constraint = IdentityConstraint::unique("id-unique", ".//item").with_field("@id");

        let mut validator = ConstraintValidator::new();

        // Null values are allowed for unique (they don't participate in uniqueness)
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single(""))
                .is_ok()
        );
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::single(""))
                .is_ok()
        );
    }

    #[test]
    fn test_keyref_validation() {
        let key = IdentityConstraint::key("category-key", ".//category").with_field("@id");
        let keyref = IdentityConstraint::keyref("item-category", ".//item", "category-key")
            .with_field("@category");

        let mut validator = ConstraintValidator::new();

        // Add key values
        validator
            .add_key_value(&key, KeyValue::single("cat1"))
            .unwrap();
        validator
            .add_key_value(&key, KeyValue::single("cat2"))
            .unwrap();

        // Add valid keyref
        validator.add_keyref_value(&keyref, KeyValue::single("cat1"));

        // Add invalid keyref
        validator.add_keyref_value(&keyref, KeyValue::single("cat3"));

        // Validate keyrefs
        let result = validator.validate_keyrefs();
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_composite_key() {
        let constraint =
            IdentityConstraint::key("composite-key", ".//item").with_fields(["@type", "@id"]);

        let mut validator = ConstraintValidator::new();

        // Add composite key
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::new(vec!["A".into(), "1".into()]))
                .is_ok()
        );

        // Same first field, different second - OK
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::new(vec!["A".into(), "2".into()]))
                .is_ok()
        );

        // Different first field, same second - OK
        assert!(
            validator
                .add_key_value(&constraint, KeyValue::new(vec!["B".into(), "1".into()]))
                .is_ok()
        );

        // Duplicate composite - should fail
        let result =
            validator.add_key_value(&constraint, KeyValue::new(vec!["A".into(), "1".into()]));
        assert!(matches!(result, Err(ConstraintError::DuplicateKey { .. })));
    }

    #[test]
    fn test_key_value_set() {
        let mut set = KeyValueSet::new("test", 1);

        assert!(set.add(KeyValue::single("a")).is_ok());
        assert!(set.add(KeyValue::single("b")).is_ok());
        assert!(set.contains(&KeyValue::single("a")));
        assert!(!set.contains(&KeyValue::single("c")));
        assert_eq!(set.len(), 2);
    }
}
