//! Fetch result type.

/// Result of a schema fetch operation.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The fetched content.
    pub content: Vec<u8>,
    /// The final URL after any redirects.
    pub final_url: String,
    /// Whether a redirect occurred.
    pub redirected: bool,
}
