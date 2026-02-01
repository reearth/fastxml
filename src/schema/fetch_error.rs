//! Fetch error types for schema fetching operations.

use std::fmt;

/// Errors that occur during schema fetching.
#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    /// HTTP request failed
    RequestFailed {
        /// The URL that was being fetched
        url: String,
        /// The error message
        message: String,
    },
    /// HTTP error response (non-2xx status)
    HttpError {
        /// HTTP status code
        status: u16,
        /// The URL that returned the error
        url: String,
    },
    /// Failed to read the response body
    ReadResponseFailed {
        /// The error message
        message: String,
    },
    /// Failed to create HTTP client
    ClientCreationFailed {
        /// The error message
        message: String,
    },
    /// Network access is disabled
    NetworkDisabled {
        /// The URL that was requested
        url: String,
    },
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::RequestFailed { url, message } => {
                write!(f, "failed to fetch {}: {}", url, message)
            }
            FetchError::HttpError { status, url } => {
                write!(f, "HTTP error {}: {}", status, url)
            }
            FetchError::ReadResponseFailed { message } => {
                write!(f, "failed to read response: {}", message)
            }
            FetchError::ClientCreationFailed { message } => {
                write!(f, "failed to create HTTP client: {}", message)
            }
            FetchError::NetworkDisabled { url } => {
                write!(f, "network access disabled, cannot fetch: {}", url)
            }
        }
    }
}

impl std::error::Error for FetchError {}
