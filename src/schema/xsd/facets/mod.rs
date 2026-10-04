//! XSD Facet validation.
//!
//! This module implements validation for XSD simple type facets:
//!
//! ## Length Facets
//! - `length` - exact length
//! - `minLength` - minimum length
//! - `maxLength` - maximum length
//!
//! ## Range Facets
//! - `minInclusive` - minimum inclusive bound
//! - `maxInclusive` - maximum inclusive bound
//! - `minExclusive` - minimum exclusive bound
//! - `maxExclusive` - maximum exclusive bound
//!
//! ## Pattern Facets
//! - `pattern` - regular expression pattern
//! - `enumeration` - allowed values
//!
//! ## Numeric Facets
//! - `totalDigits` - total number of digits
//! - `fractionDigits` - digits after decimal point
//!
//! ## Whitespace Handling
//! - `whiteSpace` - preserve, replace, or collapse

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;

use regex::Regex;

use crate::error::Result;
use crate::schema::types::{CompiledSchema, SimpleType, TypeDef};

mod pattern;

use self::pattern::compile_xsd_pattern;
use super::primitive::PrimitiveKind;
use super::value_compare::compare_values;

/// Nesting bound for list item / union member types when building
/// [`FacetConstraints`] (schemas reject cyclic definitions, so this only
/// guards against pathological depth).
const MAX_NESTED_TYPE_DEPTH: usize = 16;

/// Whitespace handling modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceHandling {
    /// Preserve all whitespace as-is
    Preserve,
    /// Replace all whitespace characters with spaces
    Replace,
    /// Collapse consecutive whitespace to single space, trim ends
    #[default]
    Collapse,
}

/// Error types for facet validation.
#[derive(Debug, Clone)]
pub enum FacetError {
    /// Value is too short
    TooShort {
        /// Actual length of the value
        value_len: usize,
        /// Minimum required length
        min_len: usize,
    },
    /// Value is too long
    TooLong {
        /// Actual length of the value
        value_len: usize,
        /// Maximum allowed length
        max_len: usize,
    },
    /// Value does not match exact length
    WrongLength {
        /// Actual length of the value
        value_len: usize,
        /// Required exact length
        required_len: usize,
    },
    /// Value is below minimum (inclusive)
    BelowMinInclusive {
        /// The value that failed validation
        value: String,
        /// The minimum bound
        min: String,
    },
    /// Value is above maximum (inclusive)
    AboveMaxInclusive {
        /// The value that failed validation
        value: String,
        /// The maximum bound
        max: String,
    },
    /// Value is at or below minimum (exclusive)
    BelowMinExclusive {
        /// The value that failed validation
        value: String,
        /// The minimum bound (exclusive)
        min: String,
    },
    /// Value is at or above maximum (exclusive)
    AboveMaxExclusive {
        /// The value that failed validation
        value: String,
        /// The maximum bound (exclusive)
        max: String,
    },
    /// Value does not match pattern
    PatternMismatch {
        /// The value that failed validation
        value: String,
        /// The pattern that did not match
        pattern: String,
    },
    /// Value is not in enumeration
    NotInEnumeration {
        /// The value that failed validation
        value: String,
        /// List of allowed values
        allowed: Vec<String>,
    },
    /// Too many total digits
    TooManyDigits {
        /// Actual number of digits found
        found: usize,
        /// Maximum allowed digits
        max: usize,
    },
    /// Too many fraction digits
    TooManyFractionDigits {
        /// Actual number of fraction digits found
        found: usize,
        /// Maximum allowed fraction digits
        max: usize,
    },
    /// Invalid pattern regex
    InvalidPattern {
        /// The pattern that is invalid
        pattern: String,
        /// Error message
        error: String,
    },
    /// A list item is not valid against the list's item type
    InvalidListItem {
        /// The offending item
        item: String,
        /// Why the item is invalid
        message: String,
    },
    /// A union value is not valid against any of the union's member types
    NoMatchingUnionMember {
        /// The value that failed validation
        value: String,
    },
}

impl std::fmt::Display for FacetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FacetError::TooShort { value_len, min_len } => {
                write!(
                    f,
                    "value length {} is less than minimum {}",
                    value_len, min_len
                )
            }
            FacetError::TooLong { value_len, max_len } => {
                write!(f, "value length {} exceeds maximum {}", value_len, max_len)
            }
            FacetError::WrongLength {
                value_len,
                required_len,
            } => {
                write!(
                    f,
                    "value length {} does not match required {}",
                    value_len, required_len
                )
            }
            FacetError::BelowMinInclusive { value, min } => {
                write!(f, "value '{}' is below minimum '{}'", value, min)
            }
            FacetError::AboveMaxInclusive { value, max } => {
                write!(f, "value '{}' exceeds maximum '{}'", value, max)
            }
            FacetError::BelowMinExclusive { value, min } => {
                write!(f, "value '{}' must be greater than '{}'", value, min)
            }
            FacetError::AboveMaxExclusive { value, max } => {
                write!(f, "value '{}' must be less than '{}'", value, max)
            }
            FacetError::PatternMismatch { value, pattern } => {
                write!(f, "value '{}' does not match pattern '{}'", value, pattern)
            }
            FacetError::NotInEnumeration { value, allowed } => {
                write!(f, "value '{}' not in allowed values: {:?}", value, allowed)
            }
            FacetError::TooManyDigits { found, max } => {
                write!(f, "value has {} digits, maximum is {}", found, max)
            }
            FacetError::TooManyFractionDigits { found, max } => {
                write!(f, "value has {} fraction digits, maximum is {}", found, max)
            }
            FacetError::InvalidPattern { pattern, error } => {
                write!(f, "invalid pattern '{}': {}", pattern, error)
            }
            FacetError::InvalidListItem { item, message } => {
                write!(f, "list item '{}': {}", item, message)
            }
            FacetError::NoMatchingUnionMember { value } => {
                write!(
                    f,
                    "value '{}' is not valid for any member type of the union",
                    value
                )
            }
        }
    }
}

impl std::error::Error for FacetError {}

/// Compiled facet constraints for a simple type.
#[derive(Debug, Clone, Default)]
pub struct FacetConstraints {
    /// Exact length constraint
    pub length: Option<usize>,
    /// Minimum length constraint
    pub min_length: Option<usize>,
    /// Maximum length constraint
    pub max_length: Option<usize>,
    /// Minimum inclusive bound (as string for comparison)
    pub min_inclusive: Option<String>,
    /// Maximum inclusive bound
    pub max_inclusive: Option<String>,
    /// Minimum exclusive bound
    pub min_exclusive: Option<String>,
    /// Maximum exclusive bound
    pub max_exclusive: Option<String>,
    /// Pattern constraints (all must match)
    pub patterns: Vec<String>,
    /// Compiled regex patterns (for efficient validation)
    pub compiled_patterns: Vec<Arc<Regex>>,
    /// Enumeration values
    pub enumeration: HashSet<String>,
    /// Total digits constraint
    pub total_digits: Option<usize>,
    /// Fraction digits constraint
    pub fraction_digits: Option<usize>,
    /// Whitespace handling mode
    pub whitespace: WhitespaceHandling,
    /// Explicit timezone requirement (XSD 1.1)
    pub explicit_timezone: Option<crate::schema::types::ExplicitTimezone>,
    /// Primitive value space the type bottoms out in (drives range
    /// comparison and length semantics). `None` for string-family types.
    pub value_kind: Option<PrimitiveKind>,
    /// Whether the type is a list type (length facets count items).
    pub is_list: bool,
    /// Primitive kind of the item type for list types.
    pub item_kind: Option<PrimitiveKind>,
    /// Constraints of a list's item type (its facets and value space), each
    /// item being validated against them. `None` when the item type has no
    /// definition to resolve; `item_kind` then still checks the items.
    pub item_constraints: Option<Arc<FacetConstraints>>,
    /// Constraints of each member type of a union: a value is valid when it
    /// is valid against at least one member. Empty for non-union types, and
    /// when a member could not be resolved (the union is then not checked).
    pub member_constraints: Vec<Arc<FacetConstraints>>,
}

impl FacetConstraints {
    /// Creates new empty facet constraints.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds facet constraints for a [`SimpleType`], walking its base-type
    /// chain so facets declared on ancestor restrictions are inherited.
    ///
    /// The most-derived declaration of each facet wins; patterns are
    /// AND-combined across derivation steps (per XSD); the most-derived
    /// non-empty enumeration replaces any inherited one. The type's primitive
    /// value space and list variety are resolved so range and length facets
    /// can be applied with the right semantics.
    pub fn from_simple_type(schema: &CompiledSchema, simple: &SimpleType) -> Self {
        Self::build(schema, simple, 0)
    }

    /// [`from_simple_type`](Self::from_simple_type), with the nesting depth
    /// of list item / union member types bounding the recursion.
    fn build(schema: &CompiledSchema, simple: &SimpleType, depth: usize) -> Self {
        let mut c = FacetConstraints::new();
        c.value_kind = PrimitiveKind::resolve(schema, simple);

        let mut declared_ws: Option<crate::schema::types::WhiteSpace> = None;
        let mut current = simple;
        for _ in 0..16 {
            if declared_ws.is_none() {
                declared_ws = current.white_space;
            }
            if c.length.is_none() {
                c.length = current.length.map(|n| n as usize);
            }
            if c.min_length.is_none() {
                c.min_length = current.min_length.map(|n| n as usize);
            }
            if c.max_length.is_none() {
                c.max_length = current.max_length.map(|n| n as usize);
            }
            if c.min_inclusive.is_none() {
                c.min_inclusive = current.min_inclusive.clone();
            }
            if c.max_inclusive.is_none() {
                c.max_inclusive = current.max_inclusive.clone();
            }
            if c.min_exclusive.is_none() {
                c.min_exclusive = current.min_exclusive.clone();
            }
            if c.max_exclusive.is_none() {
                c.max_exclusive = current.max_exclusive.clone();
            }
            if c.total_digits.is_none() {
                c.total_digits = current.total_digits.map(|n| n as usize);
            }
            if c.fraction_digits.is_none() {
                c.fraction_digits = current.fraction_digits.map(|n| n as usize);
            }
            if c.enumeration.is_empty() && !current.enumeration.is_empty() {
                c.enumeration.extend(current.enumeration.iter().cloned());
            }
            if let Some(ref p) = current.pattern {
                c.patterns.push(p.clone());
            }
            if c.explicit_timezone.is_none() {
                c.explicit_timezone = current.explicit_timezone;
            }

            if let Some(ref item_type) = current.item_type {
                c.is_list = true;
                // ns-first item-type resolution (string fallback inside
                // type_by_ref), keeping the name-shape fallback for built-ins
                // that have no definition entry. An anonymous item type
                // carries its own facets.
                let item_def = match current.item_inline.as_deref() {
                    Some(inline) => Some(inline),
                    None => match schema.type_by_ref(current.item_ns.as_ref(), item_type) {
                        Some(TypeDef::Simple(item)) => Some(item),
                        _ => None,
                    },
                };
                c.item_kind = match item_def {
                    Some(item) => PrimitiveKind::resolve(schema, item),
                    None => PrimitiveKind::from_type_name(item_type),
                };
                if depth < MAX_NESTED_TYPE_DEPTH {
                    c.item_constraints =
                        item_def.map(|item| Arc::new(Self::build(schema, item, depth + 1)));
                }
                break;
            }

            if !current.member_types.is_empty() || !current.inline_members.is_empty() {
                if depth < MAX_NESTED_TYPE_DEPTH {
                    c.member_constraints = Self::union_members(schema, current, depth);
                }
                break;
            }

            match schema.simple_base_def(current) {
                Some(TypeDef::Simple(next)) => current = next,
                _ => break,
            }
        }

        // Whitespace normalization: an explicit whiteSpace facet wins;
        // otherwise the string family preserves whitespace while every other
        // primitive (and lists) carries a fixed whiteSpace=collapse.
        c.whitespace = match declared_ws {
            Some(crate::schema::types::WhiteSpace::Preserve) => WhitespaceHandling::Preserve,
            Some(crate::schema::types::WhiteSpace::Replace) => WhitespaceHandling::Replace,
            Some(crate::schema::types::WhiteSpace::Collapse) => WhitespaceHandling::Collapse,
            None if c.value_kind.is_some() || c.is_list => WhitespaceHandling::Collapse,
            None => WhitespaceHandling::Preserve,
        };

        let _ = c.compile_patterns();
        c
    }

    /// Builds the constraints of every member type of a union, or none when
    /// some member cannot be resolved (checking against an incomplete member
    /// set would reject values of the missing member).
    fn union_members(
        schema: &CompiledSchema,
        union: &SimpleType,
        depth: usize,
    ) -> Vec<Arc<FacetConstraints>> {
        let mut members = Vec::new();
        for (i, name) in union.member_types.iter().enumerate() {
            let ns = union.member_ns.get(i).and_then(Option::as_ref);
            match schema.type_by_ref(ns, name) {
                Some(TypeDef::Simple(member)) => {
                    members.push(Arc::new(Self::build(schema, member, depth + 1)))
                }
                _ => match PrimitiveKind::from_type_name(name) {
                    Some(kind) => members.push(Arc::new(FacetConstraints {
                        value_kind: Some(kind),
                        whitespace: WhitespaceHandling::Collapse,
                        ..FacetConstraints::new()
                    })),
                    None => return Vec::new(),
                },
            }
        }
        for member in &union.inline_members {
            members.push(Arc::new(Self::build(schema, member, depth + 1)));
        }
        members
    }

    /// Sets the length constraint.
    pub fn with_length(mut self, len: usize) -> Self {
        self.length = Some(len);
        self
    }

    /// Sets the minimum length constraint.
    pub fn with_min_length(mut self, len: usize) -> Self {
        self.min_length = Some(len);
        self
    }

    /// Sets the maximum length constraint.
    pub fn with_max_length(mut self, len: usize) -> Self {
        self.max_length = Some(len);
        self
    }

    /// Sets the minimum inclusive constraint.
    pub fn with_min_inclusive(mut self, value: impl Into<String>) -> Self {
        self.min_inclusive = Some(value.into());
        self
    }

    /// Sets the maximum inclusive constraint.
    pub fn with_max_inclusive(mut self, value: impl Into<String>) -> Self {
        self.max_inclusive = Some(value.into());
        self
    }

    /// Adds a pattern constraint.
    ///
    /// Patterns are XSD-style regular expressions. Call `compile_patterns()`
    /// after adding all patterns to compile them for efficient validation.
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    /// Adds enumeration values.
    pub fn with_enumeration(mut self, values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.enumeration.extend(values.into_iter().map(Into::into));
        self
    }

    /// Sets whitespace handling mode.
    pub fn with_whitespace(mut self, mode: WhitespaceHandling) -> Self {
        self.whitespace = mode;
        self
    }

    /// Compiles patterns for efficient validation.
    ///
    /// XSD patterns are anchored (must match entire string), so we wrap them
    /// with `^` and `$` anchors. XSD-specific regex constructs (`\i`, `\c`,
    /// character class subtraction) are translated to Rust regex syntax.
    /// Patterns that still fail to compile are skipped (logged) rather than
    /// turned into validation errors. Call this after adding all patterns.
    pub fn compile_patterns(&mut self) -> Result<()> {
        self.compiled_patterns.clear();
        for pattern in &self.patterns {
            if let Some(regex) = compile_xsd_pattern(pattern) {
                self.compiled_patterns.push(Arc::new(regex));
            }
        }
        Ok(())
    }

    /// Checks if patterns have been compiled.
    pub fn patterns_compiled(&self) -> bool {
        self.patterns.is_empty() || !self.compiled_patterns.is_empty()
    }
}

/// A memoizing cache for compiled facet constraints, keyed by type name.
///
/// Building [`FacetConstraints`] walks the base-type chain and compiles
/// regex patterns, which is far too expensive to repeat per text node or
/// attribute. Named types (the common case in real schemas) are built once;
/// anonymous types fall back to a fresh build.
#[derive(Debug, Default)]
pub(crate) struct FacetCache {
    by_name: rustc_hash::FxHashMap<String, Arc<FacetConstraints>>,
}

impl FacetCache {
    /// Returns the constraints for `simple`, memoized by type name.
    pub(crate) fn get(
        &mut self,
        schema: &CompiledSchema,
        simple: &SimpleType,
    ) -> Arc<FacetConstraints> {
        if simple.name.is_empty() {
            return Arc::new(FacetConstraints::from_simple_type(schema, simple));
        }
        if let Some(cached) = self.by_name.get(&simple.name) {
            return Arc::clone(cached);
        }
        let built = Arc::new(FacetConstraints::from_simple_type(schema, simple));
        self.by_name.insert(simple.name.clone(), Arc::clone(&built));
        built
    }
}

/// Facet validator for simple type values.
pub struct FacetValidator<'a> {
    constraints: &'a FacetConstraints,
}

impl<'a> FacetValidator<'a> {
    /// Creates a new facet validator.
    pub fn new(constraints: &'a FacetConstraints) -> Self {
        Self { constraints }
    }

    /// Validates a string value against all facet constraints.
    pub fn validate(&self, value: &str) -> std::result::Result<(), FacetError> {
        let raw = value;
        // Apply whitespace handling first
        let processed = self.apply_whitespace(value);
        let value: &str = &processed;

        // Length constraints
        self.validate_length(value)?;

        // Pattern constraints
        self.validate_patterns(value)?;

        // Enumeration constraint
        self.validate_enumeration(value)?;

        // Range and digit constraints in the type's value space
        self.validate_numeric_constraints(value)?;

        // Explicit timezone requirement (XSD 1.1)
        self.validate_explicit_timezone(value)?;

        // Item-level checks for list types
        self.validate_list_items(value)?;

        // Member-type check for union types (each member normalizes the
        // value itself)
        self.validate_union_members(raw)?;

        Ok(())
    }

    /// Validates a value against both the facets and the primitive value
    /// space described by `constraints`.
    fn accepts(constraints: &FacetConstraints, value: &str) -> std::result::Result<(), String> {
        FacetValidator::new(constraints)
            .validate(value)
            .map_err(|e| e.to_string())?;
        if let Some(kind) = constraints.value_kind {
            kind.validate(value).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Checks that a union value is valid against at least one member type.
    fn validate_union_members(&self, value: &str) -> std::result::Result<(), FacetError> {
        let members = &self.constraints.member_constraints;
        if members.is_empty() || members.iter().any(|m| Self::accepts(m, value).is_ok()) {
            return Ok(());
        }
        Err(FacetError::NoMatchingUnionMember {
            value: value.trim().to_string(),
        })
    }

    /// Validates the XSD 1.1 explicitTimezone facet on temporal values.
    fn validate_explicit_timezone(&self, value: &str) -> std::result::Result<(), FacetError> {
        use crate::schema::types::ExplicitTimezone;
        let Some(req) = self.constraints.explicit_timezone else {
            return Ok(());
        };
        let has_tz = crate::schema::xsd::primitive::has_timezone(value.trim());
        match req {
            ExplicitTimezone::Required if !has_tz => Err(FacetError::PatternMismatch {
                value: value.to_string(),
                pattern: "explicitTimezone=required".to_string(),
            }),
            ExplicitTimezone::Prohibited if has_tz => Err(FacetError::PatternMismatch {
                value: value.to_string(),
                pattern: "explicitTimezone=prohibited".to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// Validates each item of a list value against the list's item type.
    fn validate_list_items(&self, value: &str) -> std::result::Result<(), FacetError> {
        if !self.constraints.is_list {
            return Ok(());
        }
        if let Some(item) = self.constraints.item_constraints.as_deref() {
            for token in value.split_whitespace() {
                if let Err(message) = Self::accepts(item, token) {
                    return Err(FacetError::InvalidListItem {
                        item: token.to_string(),
                        message,
                    });
                }
            }
            return Ok(());
        }
        let Some(item_kind) = self.constraints.item_kind else {
            return Ok(());
        };
        for item in value.split_whitespace() {
            if let Err(e) = item_kind.validate(item) {
                return Err(FacetError::InvalidListItem {
                    item: item.to_string(),
                    message: e.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Applies whitespace handling to a value.
    ///
    /// C5: returns a `Cow` that borrows the input when normalization is a
    /// no-op (the common case — most typed values, including CityGML posLists,
    /// arrive already collapsed), and otherwise builds the result in a single
    /// pass. The previous implementation always allocated (two Strings for the
    /// collapse case), even when the value was unchanged.
    fn apply_whitespace<'v>(&self, value: &'v str) -> Cow<'v, str> {
        match self.constraints.whitespace {
            WhitespaceHandling::Preserve => Cow::Borrowed(value),
            WhitespaceHandling::Replace => {
                // Replace \t, \n, \r with space.
                if value
                    .as_bytes()
                    .iter()
                    .any(|&b| b == b'\t' || b == b'\n' || b == b'\r')
                {
                    Cow::Owned(
                        value
                            .chars()
                            .map(|c| {
                                if c == '\t' || c == '\n' || c == '\r' {
                                    ' '
                                } else {
                                    c
                                }
                            })
                            .collect(),
                    )
                } else {
                    Cow::Borrowed(value)
                }
            }
            WhitespaceHandling::Collapse => collapse_whitespace(value),
        }
    }

    /// Computes the facet-relevant length of a value.
    ///
    /// Per XSD Part 2, `length`/`minLength`/`maxLength` are measured in:
    /// - list items for list types,
    /// - octets for `hexBinary` / `base64Binary`,
    /// - characters otherwise.
    ///
    /// For `QName` and `NOTATION` the length facets have no effect
    /// (XSD 1.0 errata); `None` skips the checks.
    fn facet_length(&self, value: &str) -> Option<usize> {
        if self.constraints.is_list {
            return Some(value.split_whitespace().count());
        }
        match self.constraints.value_kind {
            Some(PrimitiveKind::QName) => None,
            Some(PrimitiveKind::HexBinary) => Some(value.chars().count().div_ceil(2)),
            Some(PrimitiveKind::Base64Binary) => Some(base64_decoded_len(value)),
            _ => Some(value.chars().count()),
        }
    }

    /// Validates length constraints.
    fn validate_length(&self, value: &str) -> std::result::Result<(), FacetError> {
        let Some(len) = self.facet_length(value) else {
            return Ok(());
        };

        if let Some(exact) = self.constraints.length {
            if len != exact {
                return Err(FacetError::WrongLength {
                    value_len: len,
                    required_len: exact,
                });
            }
        }

        if let Some(min) = self.constraints.min_length {
            if len < min {
                return Err(FacetError::TooShort {
                    value_len: len,
                    min_len: min,
                });
            }
        }

        if let Some(max) = self.constraints.max_length {
            if len > max {
                return Err(FacetError::TooLong {
                    value_len: len,
                    max_len: max,
                });
            }
        }

        Ok(())
    }

    /// Validates pattern constraints.
    ///
    /// All compiled patterns must match for the value to be valid.
    /// If patterns haven't been compiled yet, they are compiled on-the-fly.
    fn validate_patterns(&self, value: &str) -> std::result::Result<(), FacetError> {
        // If no patterns defined, validation passes
        if self.constraints.patterns.is_empty() {
            return Ok(());
        }

        // Use compiled patterns if available
        if !self.constraints.compiled_patterns.is_empty() {
            for regex in &self.constraints.compiled_patterns {
                if !regex.is_match(value) {
                    return Err(FacetError::PatternMismatch {
                        value: value.to_string(),
                        pattern: regex.as_str().to_string(),
                    });
                }
            }
        } else {
            // Compile patterns on-the-fly (less efficient, but works).
            // Patterns that don't compile are unsupported constructs — skip
            // them instead of failing the value.
            for pattern in &self.constraints.patterns {
                if let Some(regex) = compile_xsd_pattern(pattern) {
                    if !regex.is_match(value) {
                        return Err(FacetError::PatternMismatch {
                            value: value.to_string(),
                            pattern: pattern.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Validates enumeration constraint.
    fn validate_enumeration(&self, value: &str) -> std::result::Result<(), FacetError> {
        if !self.constraints.enumeration.is_empty() && !self.constraints.enumeration.contains(value)
        {
            return Err(FacetError::NotInEnumeration {
                value: value.to_string(),
                allowed: self.constraints.enumeration.iter().cloned().collect(),
            });
        }
        Ok(())
    }

    /// Validates range and digit constraints in the type's value space.
    fn validate_numeric_constraints(&self, value: &str) -> std::result::Result<(), FacetError> {
        let kind = self.constraints.value_kind;

        // Min inclusive
        if let Some(ref min) = self.constraints.min_inclusive {
            if compare_values(kind, value, min) == Some(Ordering::Less) {
                return Err(FacetError::BelowMinInclusive {
                    value: value.to_string(),
                    min: min.clone(),
                });
            }
        }

        // Max inclusive
        if let Some(ref max) = self.constraints.max_inclusive {
            if compare_values(kind, value, max) == Some(Ordering::Greater) {
                return Err(FacetError::AboveMaxInclusive {
                    value: value.to_string(),
                    max: max.clone(),
                });
            }
        }

        // Min exclusive
        if let Some(ref min) = self.constraints.min_exclusive {
            if matches!(
                compare_values(kind, value, min),
                Some(Ordering::Less | Ordering::Equal)
            ) {
                return Err(FacetError::BelowMinExclusive {
                    value: value.to_string(),
                    min: min.clone(),
                });
            }
        }

        // Max exclusive
        if let Some(ref max) = self.constraints.max_exclusive {
            if matches!(
                compare_values(kind, value, max),
                Some(Ordering::Greater | Ordering::Equal)
            ) {
                return Err(FacetError::AboveMaxExclusive {
                    value: value.to_string(),
                    max: max.clone(),
                });
            }
        }

        // Total digits
        if let Some(max_digits) = self.constraints.total_digits {
            let digit_count = count_significant_digits(value);
            if digit_count > max_digits {
                return Err(FacetError::TooManyDigits {
                    found: digit_count,
                    max: max_digits,
                });
            }
        }

        // Fraction digits
        if let Some(max_fraction) = self.constraints.fraction_digits {
            let fraction_count = count_fraction_digits(value);
            if fraction_count > max_fraction {
                return Err(FacetError::TooManyFractionDigits {
                    found: fraction_count,
                    max: max_fraction,
                });
            }
        }

        Ok(())
    }
}

/// Counts significant digits in a numeric string per XSD `totalDigits`:
/// leading zeros of the integer part and trailing zeros of the fraction part
/// don't count, but trailing zeros of an integer do (e.g. "100" has 3).
fn count_significant_digits(value: &str) -> usize {
    let v = value.trim().trim_start_matches(['-', '+']);
    let (int_part, frac_part) = match v.find('.') {
        Some(p) => (&v[..p], &v[p + 1..]),
        None => (v, ""),
    };
    let int_digits = int_part.trim_start_matches('0');
    let frac_digits = frac_part.trim_end_matches('0');
    let count = int_digits.chars().filter(|c| c.is_ascii_digit()).count()
        + frac_digits.chars().filter(|c| c.is_ascii_digit()).count();
    // zero is one digit
    count.max(1)
}

/// Number of octets a base64Binary lexical value decodes to.
/// XSD `whiteSpace=collapse`: replace every whitespace char with a space,
/// collapse runs to a single space, and trim leading/trailing whitespace.
///
/// Returns a borrow when the value is already collapsed (no leading/trailing
/// whitespace, no consecutive whitespace, and every whitespace char is a plain
/// U+0020 space) — otherwise builds the normalized form in a single pass.
/// Byte-for-byte identical output to the previous two-String implementation.
fn collapse_whitespace(value: &str) -> Cow<'_, str> {
    // Fast path: is normalization a no-op?
    let mut prev_space = true; // start true so a leading whitespace is caught
    let mut needs_build = false;
    for c in value.chars() {
        if c.is_whitespace() {
            if c != ' ' || prev_space {
                needs_build = true;
                break;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
    }
    if !needs_build && !value.ends_with(' ') {
        return Cow::Borrowed(value);
    }

    // Single-pass build.
    let mut out = String::with_capacity(value.len());
    let mut prev_space = true; // start true to trim leading whitespace
    for c in value.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    Cow::Owned(out)
}

fn base64_decoded_len(value: &str) -> usize {
    let chars: Vec<char> = value.chars().filter(|c| !c.is_whitespace()).collect();
    let padding = chars.iter().rev().take_while(|&&c| c == '=').count();
    let n = chars.len();
    if n.is_multiple_of(4) {
        ((n / 4) * 3).saturating_sub(padding.min(2))
    } else {
        // Not valid base64; approximate so length facets stay sane.
        n * 3 / 4
    }
}

/// Counts fraction digits (digits after decimal point).
fn count_fraction_digits(value: &str) -> usize {
    if let Some(pos) = value.find('.') {
        value[pos + 1..]
            .chars()
            .filter(|c| c.is_ascii_digit())
            .count()
    } else {
        0
    }
}

#[cfg(test)]
mod tests;
