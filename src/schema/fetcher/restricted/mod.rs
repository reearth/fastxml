//! Fetcher wrappers that only fetch locations allowed by a [`FetchPolicy`].

mod policy;

pub use policy::FetchPolicy;

use std::path::Path;

use crate::error::Result;

use super::{FetchResult, SchemaFetcher};

/// A fetcher wrapper that only lets through locations allowed by a
/// [`FetchPolicy`].
///
/// When a schema is auto-detected, the locations fetched are chosen by the
/// validated document (`xsi:schemaLocation`) and by the schemas it names
/// (`xs:import` / `xs:include` / `xs:redefine`). Wrap the fetcher in a
/// `RestrictedFetcher` when those documents are not trusted, so they cannot
/// make the validator read arbitrary local files or contact arbitrary hosts.
///
/// Each location is checked before the inner fetcher sees it; a rejected
/// location never reaches the inner fetcher and fails with
/// [`FetchError::NotAllowed`](super::error::FetchError::NotAllowed). See
/// [`FetchPolicy`] for the matching rules.
///
/// # Redirects
///
/// If the inner fetcher follows a redirect, the final URL it reports is
/// checked as well and the result is discarded when that URL is outside the
/// policy. The redirected request itself has already been made by then: the
/// HTTP fetchers in this crate follow redirects internally, so a server under
/// an allowed prefix can still make the client *contact* a disallowed host
/// (though its response is never used). To prevent that, configure the inner
/// fetcher not to follow redirects, e.g. `UreqFetcher::new().max_redirects(0)`.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{FileFetcher, RestrictedFetcher, SchemaFetcher};
///
/// let fetcher = RestrictedFetcher::new(FileFetcher::new())
///     .allow_url_prefix("https://schemas.opengis.net/")
///     .allow_dir("./schemas");
///
/// assert!(fetcher.fetch("/etc/passwd").is_err());
/// let schema = fetcher.fetch("./schemas/building.xsd")?;
/// # Ok::<(), fastxml::error::Error>(())
/// ```
///
/// Auto-detecting the schema of an untrusted document:
///
/// ```no_run
/// # #[cfg(feature = "ureq")]
/// # fn main() -> fastxml::error::Result<()> {
/// use fastxml::schema::{DefaultFetcher, Validator};
/// use fastxml::schema::fetcher::RestrictedFetcher;
///
/// # let xml = "";
/// let fetcher = RestrictedFetcher::new(DefaultFetcher::new())
///     .allow_url_prefix("https://schemas.opengis.net/")
///     .allow_dir("/srv/schemas");
/// let report = Validator::from(xml).run_with(fetcher)?;
/// # Ok(())
/// # }
/// # #[cfg(not(feature = "ureq"))]
/// # fn main() {}
/// ```
pub struct RestrictedFetcher<F> {
    inner: F,
    policy: FetchPolicy,
}

impl<F> RestrictedFetcher<F> {
    /// Wraps `inner` with an empty policy, which allows nothing until
    /// locations are added with [`allow_url_prefix`](Self::allow_url_prefix)
    /// or [`allow_dir`](Self::allow_dir).
    pub fn new(inner: F) -> Self {
        Self::with_policy(inner, FetchPolicy::new())
    }

    /// Wraps `inner` with the given policy.
    pub fn with_policy(inner: F, policy: FetchPolicy) -> Self {
        Self { inner, policy }
    }

    /// Allows `http`/`https` URLs under `prefix`. See
    /// [`FetchPolicy::allow_url_prefix`].
    pub fn allow_url_prefix(mut self, prefix: &str) -> Self {
        self.policy = self.policy.allow_url_prefix(prefix);
        self
    }

    /// Allows local files inside `dir`. See [`FetchPolicy::allow_dir`].
    pub fn allow_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.policy = self.policy.allow_dir(dir);
        self
    }

    /// Sets the directory relative local paths are resolved against. See
    /// [`FetchPolicy::base_dir`].
    pub fn base_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.policy = self.policy.base_dir(dir);
        self
    }

    /// Returns the policy.
    pub fn policy(&self) -> &FetchPolicy {
        &self.policy
    }

    /// Returns the wrapped fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }
}

impl<F: SchemaFetcher> SchemaFetcher for RestrictedFetcher<F> {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        let location = self.policy.check(url)?;
        let result = self.inner.fetch(&location)?;
        check_final_url(&self.policy, &location, &result)?;
        Ok(result)
    }
}

/// Rejects a result whose final URL (after redirects) is outside the policy.
fn check_final_url(policy: &FetchPolicy, requested: &str, result: &FetchResult) -> Result<()> {
    if result.redirected || result.final_url != requested {
        policy.check(&result.final_url)?;
    }
    Ok(())
}

/// Async version of [`RestrictedFetcher`] (requires the `tokio` feature).
///
/// The same [`FetchPolicy`] rules and redirect caveat apply. The policy check
/// canonicalizes local paths with blocking filesystem calls.
///
/// # Examples
///
/// ```no_run
/// use fastxml::schema::fetcher::{AsyncDefaultFetcher, AsyncRestrictedFetcher};
///
/// # fn example() -> fastxml::error::Result<()> {
/// let fetcher = AsyncRestrictedFetcher::new(AsyncDefaultFetcher::new()?)
///     .allow_url_prefix("https://schemas.opengis.net/")
///     .allow_dir("./schemas");
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "tokio")]
pub struct AsyncRestrictedFetcher<F> {
    inner: F,
    policy: FetchPolicy,
}

#[cfg(feature = "tokio")]
impl<F> AsyncRestrictedFetcher<F> {
    /// Wraps `inner` with an empty policy, which allows nothing.
    pub fn new(inner: F) -> Self {
        Self::with_policy(inner, FetchPolicy::new())
    }

    /// Wraps `inner` with the given policy.
    pub fn with_policy(inner: F, policy: FetchPolicy) -> Self {
        Self { inner, policy }
    }

    /// Allows `http`/`https` URLs under `prefix`. See
    /// [`FetchPolicy::allow_url_prefix`].
    pub fn allow_url_prefix(mut self, prefix: &str) -> Self {
        self.policy = self.policy.allow_url_prefix(prefix);
        self
    }

    /// Allows local files inside `dir`. See [`FetchPolicy::allow_dir`].
    pub fn allow_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.policy = self.policy.allow_dir(dir);
        self
    }

    /// Sets the directory relative local paths are resolved against. See
    /// [`FetchPolicy::base_dir`].
    pub fn base_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.policy = self.policy.base_dir(dir);
        self
    }

    /// Returns the policy.
    pub fn policy(&self) -> &FetchPolicy {
        &self.policy
    }

    /// Returns the wrapped fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }
}

#[cfg(feature = "tokio")]
#[async_trait::async_trait]
impl<F: super::AsyncSchemaFetcher> super::AsyncSchemaFetcher for AsyncRestrictedFetcher<F> {
    async fn fetch(&self, url: &str) -> Result<FetchResult> {
        let location = self.policy.check(url)?;
        let result = self.inner.fetch(&location).await?;
        check_final_url(&self.policy, &location, &result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
