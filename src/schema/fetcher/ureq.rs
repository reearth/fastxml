//! Sync schema fetcher using ureq.

use std::io::Read;

use super::error::FetchError;
use crate::error::Result;

use super::{FetchResult, SchemaFetcher};

/// Sync schema fetcher using ureq.
pub struct UreqFetcher {
    /// Maximum number of redirects to follow.
    max_redirects: u32,
    /// User agent string.
    user_agent: String,
    /// Timeout in seconds.
    timeout_secs: u64,
}

impl UreqFetcher {
    /// Creates a new fetcher with default settings.
    pub fn new() -> Self {
        Self {
            max_redirects: 10,
            user_agent: format!("fastxml/{}", env!("CARGO_PKG_VERSION")),
            timeout_secs: 30,
        }
    }

    /// Sets the maximum number of redirects.
    pub fn max_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Sets the user agent string.
    pub fn user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = agent.into();
        self
    }

    /// Sets the timeout in seconds.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    fn build_agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .redirects(self.max_redirects)
            .build()
    }
}

impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaFetcher for UreqFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let agent = self.build_agent();

        let response = agent
            .get(url)
            .set("User-Agent", &self.user_agent)
            .call()
            .map_err(|e| FetchError::RequestFailed {
                url: url.to_string(),
                message: e.to_string(),
            })?;

        let status = response.status();
        let final_url = response.get_url().to_string();
        let redirected = final_url != url;

        if status != 200 {
            return Err(FetchError::HttpError {
                status,
                url: url.to_string(),
            }
            .into());
        }

        // Read content
        let mut content = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut content)
            .map_err(|e| FetchError::ReadResponseFailed {
                message: e.to_string(),
            })?;

        Ok(FetchResult {
            content,
            final_url,
            redirected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ureq_fetcher_new() {
        let fetcher = UreqFetcher::new();
        assert_eq!(fetcher.max_redirects, 10);
        assert!(fetcher.user_agent.contains("fastxml"));
        assert_eq!(fetcher.timeout_secs, 30);
    }

    #[test]
    fn test_ureq_fetcher_default() {
        let fetcher = UreqFetcher::default();
        assert_eq!(fetcher.max_redirects, 10);
        assert!(fetcher.user_agent.contains("fastxml"));
        assert_eq!(fetcher.timeout_secs, 30);
    }

    #[test]
    fn test_ureq_fetcher_builder_max_redirects() {
        let fetcher = UreqFetcher::new().max_redirects(5);
        assert_eq!(fetcher.max_redirects, 5);
    }

    #[test]
    fn test_ureq_fetcher_builder_user_agent() {
        let fetcher = UreqFetcher::new().user_agent("custom-agent/1.0");
        assert_eq!(fetcher.user_agent, "custom-agent/1.0");
    }

    #[test]
    fn test_ureq_fetcher_builder_timeout() {
        let fetcher = UreqFetcher::new().timeout(60);
        assert_eq!(fetcher.timeout_secs, 60);
    }

    #[test]
    fn test_ureq_fetcher_builder_chain() {
        let fetcher = UreqFetcher::new()
            .max_redirects(3)
            .user_agent("test-agent")
            .timeout(15);
        assert_eq!(fetcher.max_redirects, 3);
        assert_eq!(fetcher.user_agent, "test-agent");
        assert_eq!(fetcher.timeout_secs, 15);
    }

    #[test]
    fn test_ureq_fetcher_build_agent() {
        let fetcher = UreqFetcher::new();
        // Just verify build_agent doesn't panic
        let _agent = fetcher.build_agent();
    }

    #[test]
    fn test_ureq_fetcher_fetch_invalid_url() {
        let fetcher = UreqFetcher::new();
        let result = fetcher.fetch("not-a-valid-url");
        assert!(result.is_err());
    }
}
