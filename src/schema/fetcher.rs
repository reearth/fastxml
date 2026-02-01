//! Schema fetching with redirect support.

#[cfg(feature = "ureq")]
use std::io::Read;

use crate::error::Result;
use crate::schema::fetch_error::FetchError;

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

/// Trait for fetching schemas from URLs.
pub trait SchemaFetcher: Send + Sync {
    /// Fetches a schema from the given URL.
    ///
    /// Follows redirects automatically and returns the final URL.
    fn fetch(&self, url: &str) -> Result<FetchResult>;
}

/// Sync schema fetcher using ureq.
#[cfg(feature = "ureq")]
pub struct UreqFetcher {
    /// Maximum number of redirects to follow.
    max_redirects: u32,
    /// User agent string.
    user_agent: String,
    /// Timeout in seconds.
    timeout_secs: u64,
}

#[cfg(feature = "ureq")]
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

#[cfg(feature = "ureq")]
impl Default for UreqFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "ureq")]
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

/// Async schema fetcher using reqwest.
#[cfg(feature = "reqwest")]
pub struct ReqwestFetcher {
    client: reqwest::Client,
}

#[cfg(feature = "reqwest")]
impl ReqwestFetcher {
    /// Creates a new fetcher with default settings.
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(format!("fastxml/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| FetchError::ClientCreationFailed {
                message: e.to_string(),
            })?;

        Ok(Self { client })
    }

    /// Creates a fetcher with a custom reqwest client.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Fetches a schema asynchronously.
    pub async fn fetch_async(&self, url: &str) -> Result<FetchResult> {
        let response =
            self.client
                .get(url)
                .send()
                .await
                .map_err(|e| FetchError::RequestFailed {
                    url: url.to_string(),
                    message: e.to_string(),
                })?;

        let final_url = response.url().to_string();
        let redirected = final_url != url;

        if !response.status().is_success() {
            return Err(FetchError::HttpError {
                status: response.status().as_u16(),
                url: url.to_string(),
            }
            .into());
        }

        let content = response
            .bytes()
            .await
            .map_err(|e| FetchError::ReadResponseFailed {
                message: e.to_string(),
            })?
            .to_vec();

        Ok(FetchResult {
            content,
            final_url,
            redirected,
        })
    }
}

#[cfg(feature = "reqwest")]
impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new().expect("failed to create HTTP client")
    }
}

/// A no-op fetcher that always fails.
///
/// Useful for testing or when network access is disabled.
pub struct NoopFetcher;

impl SchemaFetcher for NoopFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        Err(FetchError::NetworkDisabled {
            url: url.to_string(),
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_fetcher() {
        let fetcher = NoopFetcher;
        let result = fetcher.fetch("http://example.com/schema.xsd");
        assert!(result.is_err());
    }

    #[test]
    fn test_noop_fetcher_error_message() {
        let fetcher = NoopFetcher;
        let result = fetcher.fetch("http://example.com/test.xsd");
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("network"));
        assert!(msg.contains("http://example.com/test.xsd"));
    }

    #[test]
    fn test_fetch_result_struct() {
        let result = FetchResult {
            content: vec![1, 2, 3],
            final_url: "http://example.com/final.xsd".to_string(),
            redirected: true,
        };
        assert_eq!(result.content, vec![1, 2, 3]);
        assert_eq!(result.final_url, "http://example.com/final.xsd");
        assert!(result.redirected);
    }

    #[test]
    fn test_fetch_result_no_redirect() {
        let result = FetchResult {
            content: b"<schema/>".to_vec(),
            final_url: "http://example.com/schema.xsd".to_string(),
            redirected: false,
        };
        assert_eq!(result.content, b"<schema/>");
        assert!(!result.redirected);
    }

    #[test]
    fn test_fetch_result_clone() {
        let result = FetchResult {
            content: vec![42],
            final_url: "http://example.com".to_string(),
            redirected: false,
        };
        let cloned = result.clone();
        assert_eq!(cloned.content, result.content);
        assert_eq!(cloned.final_url, result.final_url);
        assert_eq!(cloned.redirected, result.redirected);
    }

    #[test]
    fn test_fetch_result_debug() {
        let result = FetchResult {
            content: vec![],
            final_url: "http://test.com".to_string(),
            redirected: true,
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("FetchResult"));
        assert!(debug.contains("http://test.com"));
        assert!(debug.contains("true"));
    }

    #[cfg(feature = "ureq")]
    mod ureq_tests {
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

    #[cfg(feature = "reqwest")]
    mod reqwest_tests {
        use super::*;

        #[test]
        fn test_reqwest_fetcher_new() {
            let fetcher = ReqwestFetcher::new();
            assert!(fetcher.is_ok());
        }

        #[test]
        fn test_reqwest_fetcher_default() {
            let _fetcher = ReqwestFetcher::default();
        }

        #[test]
        fn test_reqwest_fetcher_with_client() {
            let client = reqwest::Client::new();
            let _fetcher = ReqwestFetcher::with_client(client);
        }
    }
}
