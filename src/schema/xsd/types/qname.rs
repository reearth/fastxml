//! Qualified name type for XSD.

/// Qualified name with optional namespace prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    /// Namespace prefix (if any)
    pub prefix: Option<String>,
    /// Local name
    pub local: String,
    /// Namespace URI this QName resolved to against the namespace
    /// declarations in scope on the schema element that carried it (its own
    /// declarations over its ancestors', innermost wins), captured at parse
    /// time.
    ///
    /// For a prefixed QName this is the URI bound to the prefix, or `None`
    /// when the prefix is not declared in scope. For an unprefixed QName it
    /// is the default namespace in scope, or `Some("")` when there is none.
    /// `None` on a QName built by hand (via [`QName::new`],
    /// [`QName::with_prefix`] or [`QName::parse`]): resolution then falls
    /// back to the schema document's root bindings
    /// ([`XsdSchema::namespace_bindings`](super::XsdSchema::namespace_bindings)).
    pub namespace: Option<String>,
}

impl QName {
    /// Creates a new QName with just a local name.
    pub fn new(local: impl Into<String>) -> Self {
        Self {
            prefix: None,
            local: local.into(),
            namespace: None,
        }
    }

    /// Creates a new QName with prefix and local name.
    pub fn with_prefix(prefix: impl Into<String>, local: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            local: local.into(),
            namespace: None,
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

    /// The namespace URI the prefix (or, unprefixed, the default namespace)
    /// is bound to: the in-scope binding captured at parse time
    /// ([`namespace`](Self::namespace)) when known, otherwise `bindings` (a
    /// document's root bindings). `None` means an undeclared prefix, or no
    /// default namespace for an unprefixed QName. The `xml` prefix is not
    /// special-cased here.
    pub fn bound_namespace<'a>(
        &'a self,
        bindings: &'a std::collections::HashMap<String, String>,
    ) -> Option<&'a str> {
        let ns = match &self.namespace {
            Some(ns) => Some(ns.as_str()),
            None => {
                let prefix = self.prefix.as_deref().map(str::trim).unwrap_or("");
                bindings.get(prefix).map(String::as_str)
            }
        };
        // An unprefixed QName with no default namespace in scope has none.
        ns.filter(|ns| self.prefix.is_some() || !ns.is_empty())
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
