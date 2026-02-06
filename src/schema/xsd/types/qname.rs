//! Qualified name type for XSD.

/// Qualified name with optional namespace prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    /// Namespace prefix (if any)
    pub prefix: Option<String>,
    /// Local name
    pub local: String,
}

impl QName {
    /// Creates a new QName with just a local name.
    pub fn new(local: impl Into<String>) -> Self {
        Self {
            prefix: None,
            local: local.into(),
        }
    }

    /// Creates a new QName with prefix and local name.
    pub fn with_prefix(prefix: impl Into<String>, local: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            local: local.into(),
        }
    }

    /// Parses a QName from a string like "prefix:local" or "local".
    pub fn parse(s: &str) -> Self {
        if let Some((prefix, local)) = s.split_once(':') {
            Self::with_prefix(prefix, local)
        } else {
            Self::new(s)
        }
    }

    /// Returns the full qualified name as a string.
    pub fn to_string_full(&self) -> String {
        match &self.prefix {
            Some(p) => format!("{}:{}", p, self.local),
            None => self.local.clone(),
        }
    }
}

impl std::fmt::Display for QName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.prefix {
            Some(p) => write!(f, "{}:{}", p, self.local),
            None => write!(f, "{}", self.local),
        }
    }
}
