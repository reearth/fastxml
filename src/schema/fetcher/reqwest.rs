//! Async schema fetcher using reqwest.

use crate::error::Result;
use crate::schema::fetch_error::FetchError;

use super::FetchResult;

/// Async schema fetcher using reqwest.
pub struct ReqwestFetcher {
    client: reqwest::Client,
}

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

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self::new().expect("failed to create HTTP client")
    }
}

#[cfg(test)]
mod tests {
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
