//! Occurrence bounds for XSD elements and attributes.

/// Occurrence bounds for elements and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occurs {
    /// A specific count
    Count(u32),
    /// Unbounded (infinite)
    Unbounded,
}

impl Default for Occurs {
    fn default() -> Self {
        Occurs::Count(1)
    }
}

impl Occurs {
    /// Returns true if this represents unbounded.
    pub fn is_unbounded(&self) -> bool {
        matches!(self, Occurs::Unbounded)
    }

    /// Converts to an `Option<u32>` where None means unbounded.
    pub fn to_option(&self) -> Option<u32> {
        match self {
            Occurs::Count(n) => Some(*n),
            Occurs::Unbounded => None,
        }
    }

    /// Parses from a string, handling "unbounded".
    /// Returns an error for invalid values (non-numeric, negative in string form).
    pub fn parse(s: &str) -> Result<Self, String> {
        if s == "unbounded" {
            Ok(Occurs::Unbounded)
        } else {
            // Check for negative values (string starts with '-')
            if s.starts_with('-') {
                return Err(format!(
                    "invalid occurs value '{}': negative values not allowed",
                    s
                ));
            }
            match s.parse::<u32>() {
                Ok(n) => Ok(Occurs::Count(n)),
                // XSD places no upper limit on occurs values; clamp counts
                // beyond u32 instead of rejecting the (valid) schema.
                Err(_) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
                    Ok(Occurs::Count(u32::MAX))
                }
                Err(_) => Err(format!(
                    "invalid occurs value '{}': must be a non-negative integer or 'unbounded'",
                    s
                )),
            }
        }
    }
}
