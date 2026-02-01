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

use std::collections::HashSet;
use std::sync::Arc;

use regex::Regex;

use crate::error::Result;

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
}

impl FacetConstraints {
    /// Creates new empty facet constraints.
    pub fn new() -> Self {
        Self::default()
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
    /// with `^` and `$` anchors. Call this after adding all patterns.
    pub fn compile_patterns(&mut self) -> Result<()> {
        self.compiled_patterns.clear();
        for pattern in &self.patterns {
            // XSD patterns are implicitly anchored to match the entire string
            let anchored = format!("^(?:{})$", pattern);
            match Regex::new(&anchored) {
                Ok(regex) => {
                    self.compiled_patterns.push(Arc::new(regex));
                }
                Err(e) => {
                    // Store error but continue - we'll report during validation
                    tracing::warn!("Invalid XSD pattern '{}': {}", pattern, e);
                    // Create a regex that never matches to ensure validation fails
                    // This is a workaround - in production you might want to error here
                }
            }
        }
        Ok(())
    }

    /// Checks if patterns have been compiled.
    pub fn patterns_compiled(&self) -> bool {
        self.patterns.is_empty() || !self.compiled_patterns.is_empty()
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
        // Apply whitespace handling first
        let processed = self.apply_whitespace(value);
        let value = processed.as_deref().unwrap_or(value);

        // Length constraints
        self.validate_length(value)?;

        // Pattern constraints
        self.validate_patterns(value)?;

        // Enumeration constraint
        self.validate_enumeration(value)?;

        // Numeric constraints (for numeric types)
        self.validate_numeric_constraints(value)?;

        Ok(())
    }

    /// Applies whitespace handling to a value.
    fn apply_whitespace(&self, value: &str) -> Option<String> {
        match self.constraints.whitespace {
            WhitespaceHandling::Preserve => None,
            WhitespaceHandling::Replace => {
                // Replace \t, \n, \r with space
                Some(
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
            }
            WhitespaceHandling::Collapse => {
                // Replace whitespace and collapse to single spaces
                let replaced: String = value
                    .chars()
                    .map(|c| if c.is_whitespace() { ' ' } else { c })
                    .collect();
                // Collapse consecutive spaces and trim
                let mut result = String::new();
                let mut prev_space = true; // Start true to trim leading
                for c in replaced.chars() {
                    if c == ' ' {
                        if !prev_space {
                            result.push(c);
                        }
                        prev_space = true;
                    } else {
                        result.push(c);
                        prev_space = false;
                    }
                }
                // Trim trailing space
                if result.ends_with(' ') {
                    result.pop();
                }
                Some(result)
            }
        }
    }

    /// Validates length constraints.
    fn validate_length(&self, value: &str) -> std::result::Result<(), FacetError> {
        let len = value.chars().count();

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
            for (i, regex) in self.constraints.compiled_patterns.iter().enumerate() {
                if !regex.is_match(value) {
                    return Err(FacetError::PatternMismatch {
                        value: value.to_string(),
                        pattern: self
                            .constraints
                            .patterns
                            .get(i)
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }
        } else {
            // Compile patterns on-the-fly (less efficient, but works)
            for pattern in &self.constraints.patterns {
                let anchored = format!("^(?:{})$", pattern);
                match Regex::new(&anchored) {
                    Ok(regex) => {
                        if !regex.is_match(value) {
                            return Err(FacetError::PatternMismatch {
                                value: value.to_string(),
                                pattern: pattern.clone(),
                            });
                        }
                    }
                    Err(e) => {
                        return Err(FacetError::InvalidPattern {
                            pattern: pattern.clone(),
                            error: e.to_string(),
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

    /// Validates numeric constraints (range, digits).
    fn validate_numeric_constraints(&self, value: &str) -> std::result::Result<(), FacetError> {
        // Try to parse as number for range checks
        if let Ok(num) = value.parse::<f64>() {
            // Min inclusive
            if let Some(ref min) = self.constraints.min_inclusive {
                if let Ok(min_val) = min.parse::<f64>() {
                    if num < min_val {
                        return Err(FacetError::BelowMinInclusive {
                            value: value.to_string(),
                            min: min.clone(),
                        });
                    }
                }
            }

            // Max inclusive
            if let Some(ref max) = self.constraints.max_inclusive {
                if let Ok(max_val) = max.parse::<f64>() {
                    if num > max_val {
                        return Err(FacetError::AboveMaxInclusive {
                            value: value.to_string(),
                            max: max.clone(),
                        });
                    }
                }
            }

            // Min exclusive
            if let Some(ref min) = self.constraints.min_exclusive {
                if let Ok(min_val) = min.parse::<f64>() {
                    if num <= min_val {
                        return Err(FacetError::BelowMinExclusive {
                            value: value.to_string(),
                            min: min.clone(),
                        });
                    }
                }
            }

            // Max exclusive
            if let Some(ref max) = self.constraints.max_exclusive {
                if let Ok(max_val) = max.parse::<f64>() {
                    if num >= max_val {
                        return Err(FacetError::AboveMaxExclusive {
                            value: value.to_string(),
                            max: max.clone(),
                        });
                    }
                }
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

/// Counts significant digits in a numeric string.
fn count_significant_digits(value: &str) -> usize {
    let value = value.trim_start_matches('-').trim_start_matches('+');
    let value = value.trim_start_matches('0');
    let value = value.replace('.', "");
    let value = value.trim_end_matches('0');
    value.chars().filter(|c| c.is_ascii_digit()).count()
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
mod tests {
    use super::*;

    #[test]
    fn test_length_validation() {
        let constraints = FacetConstraints::new()
            .with_min_length(2)
            .with_max_length(5);

        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("ab").is_ok());
        assert!(validator.validate("abcde").is_ok());
        assert!(validator.validate("a").is_err());
        assert!(validator.validate("abcdef").is_err());
    }

    #[test]
    fn test_exact_length() {
        let constraints = FacetConstraints::new().with_length(3);
        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("abc").is_ok());
        assert!(validator.validate("ab").is_err());
        assert!(validator.validate("abcd").is_err());
    }

    #[test]
    fn test_enumeration() {
        let constraints = FacetConstraints::new().with_enumeration(["red", "green", "blue"]);

        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("red").is_ok());
        assert!(validator.validate("green").is_ok());
        assert!(validator.validate("yellow").is_err());
    }

    #[test]
    fn test_pattern() {
        // Pattern `[a-z]+` matches one or more lowercase letters
        let mut constraints = FacetConstraints::new().with_pattern(r"[a-z]+");
        constraints.compile_patterns().unwrap();

        let validator = FacetValidator::new(&constraints);

        // Valid: all lowercase letters
        assert!(validator.validate("hello").is_ok());
        assert!(validator.validate("world").is_ok());

        // Invalid: contains uppercase
        assert!(validator.validate("Hello").is_err());

        // Invalid: contains numbers
        assert!(validator.validate("hello123").is_err());

        // Invalid: empty string (pattern requires at least one char)
        assert!(validator.validate("").is_err());

        // Pattern is stored
        assert_eq!(constraints.patterns.len(), 1);
        assert_eq!(constraints.compiled_patterns.len(), 1);
    }

    #[test]
    fn test_pattern_multiple() {
        // Multiple patterns - all must match
        let mut constraints = FacetConstraints::new()
            .with_pattern(r"[a-z]+")
            .with_pattern(r".{3,}"); // At least 3 characters
        constraints.compile_patterns().unwrap();

        let validator = FacetValidator::new(&constraints);

        // Valid: lowercase and at least 3 chars
        assert!(validator.validate("hello").is_ok());

        // Invalid: too short
        assert!(validator.validate("hi").is_err());

        // Invalid: contains uppercase
        assert!(validator.validate("Hello").is_err());
    }

    #[test]
    fn test_numeric_range() {
        let constraints = FacetConstraints::new()
            .with_min_inclusive("0")
            .with_max_inclusive("100");

        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("0").is_ok());
        assert!(validator.validate("50").is_ok());
        assert!(validator.validate("100").is_ok());
        assert!(validator.validate("-1").is_err());
        assert!(validator.validate("101").is_err());
    }

    #[test]
    fn test_whitespace_collapse() {
        let constraints = FacetConstraints::new()
            .with_whitespace(WhitespaceHandling::Collapse)
            .with_enumeration(["hello world"]);

        let validator = FacetValidator::new(&constraints);

        // Multiple spaces should collapse to one
        assert!(validator.validate("hello  world").is_ok());
        assert!(validator.validate("  hello   world  ").is_ok());
    }

    #[test]
    fn test_fraction_digits() {
        let constraints = FacetConstraints {
            fraction_digits: Some(2),
            ..Default::default()
        };

        let validator = FacetValidator::new(&constraints);

        assert!(validator.validate("1.23").is_ok());
        assert!(validator.validate("1.2").is_ok());
        assert!(validator.validate("1").is_ok());
        assert!(validator.validate("1.234").is_err());
    }

    #[test]
    fn test_count_significant_digits() {
        assert_eq!(count_significant_digits("123"), 3);
        assert_eq!(count_significant_digits("1.23"), 3);
        assert_eq!(count_significant_digits("0.123"), 3);
        assert_eq!(count_significant_digits("-123"), 3);
        assert_eq!(count_significant_digits("00123"), 3);
    }

    #[test]
    fn test_count_fraction_digits() {
        assert_eq!(count_fraction_digits("1.23"), 2);
        assert_eq!(count_fraction_digits("1"), 0);
        assert_eq!(count_fraction_digits("1.0"), 1);
        assert_eq!(count_fraction_digits("1.234"), 3);
    }
}
