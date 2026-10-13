use std::sync::Mutex;

use super::*;
use crate::error::Error;
use crate::schema::fetcher::FileFetcher;
use crate::schema::fetcher::error::FetchError;

/// Records every location it is asked for and answers with a fixed
/// `final_url` (the requested one when `None`).
struct MockFetcher {
    calls: Mutex<Vec<String>>,
    redirect_to: Option<String>,
}

impl MockFetcher {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            redirect_to: None,
        }
    }

    fn redirecting_to(url: &str) -> Self {
        Self {
            redirect_to: Some(url.to_string()),
            ..Self::new()
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn respond(&self, url: &str) -> FetchResult {
        self.calls.lock().unwrap().push(url.to_string());
        let final_url = self.redirect_to.clone().unwrap_or_else(|| url.to_string());
        FetchResult {
            content: b"<schema/>".to_vec(),
            redirected: final_url != url,
            final_url,
        }
    }
}

impl SchemaFetcher for MockFetcher {
    fn fetch(&self, url: &str) -> Result<FetchResult> {
        Ok(self.respond(url))
    }
}

fn is_not_allowed(err: &Error) -> bool {
    matches!(err, Error::Fetch(FetchError::NotAllowed { .. }))
}

#[test]
fn allowed_url_passes_through() {
    let f = RestrictedFetcher::new(MockFetcher::new()).allow_url_prefix("https://example.com/s/");
    let r = f.fetch("https://example.com/s/a.xsd").unwrap();
    assert_eq!(r.content, b"<schema/>");
    assert_eq!(f.inner().calls(), vec!["https://example.com/s/a.xsd"]);
}

#[test]
fn disallowed_url_never_reaches_inner_fetcher() {
    let f = RestrictedFetcher::new(MockFetcher::new()).allow_url_prefix("https://example.com/s/");
    for url in [
        "https://example.com.evil.com/s/a.xsd",
        "https://example.com/s/../secret",
        "http://example.com/s/a.xsd",
        "/etc/passwd",
        "file:///etc/passwd",
    ] {
        let err = f.fetch(url).unwrap_err();
        assert!(is_not_allowed(&err), "{url}: {err}");
        assert!(err.to_string().contains(url), "{err}");
    }
    assert!(f.inner().calls().is_empty());
}

#[test]
fn redirect_outside_policy_is_rejected() {
    let f = RestrictedFetcher::new(MockFetcher::redirecting_to("https://evil.com/a.xsd"))
        .allow_url_prefix("https://example.com/");
    let err = f.fetch("https://example.com/a.xsd").unwrap_err();
    assert!(is_not_allowed(&err), "{err}");
    assert!(err.to_string().contains("https://evil.com/a.xsd"), "{err}");
}

#[test]
fn redirect_to_local_file_is_rejected() {
    let f = RestrictedFetcher::new(MockFetcher::redirecting_to("file:///etc/passwd"))
        .allow_url_prefix("https://example.com/");
    assert!(f.fetch("https://example.com/a.xsd").is_err());
}

#[test]
fn redirect_inside_policy_is_accepted() {
    let f = RestrictedFetcher::new(MockFetcher::redirecting_to("https://example.com/b/a.xsd"))
        .allow_url_prefix("https://example.com/");
    let r = f.fetch("https://example.com/a.xsd").unwrap();
    assert_eq!(r.final_url, "https://example.com/b/a.xsd");
}

#[test]
fn relative_path_is_handed_to_inner_as_checked_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.xsd"), "<a/>").unwrap();
    let f = RestrictedFetcher::new(MockFetcher::new())
        .allow_dir(tmp.path())
        .base_dir(tmp.path());
    f.fetch("a.xsd").unwrap();
    assert_eq!(
        f.inner().calls(),
        vec![tmp.path().join("a.xsd").display().to_string()]
    );
}

#[test]
fn file_url_is_handed_to_inner_with_a_lowercase_scheme() {
    // `FileFetcher` only strips a lowercase `file://`; any other spelling
    // would be read as a relative path, i.e. not the file that was checked.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.xsd"), "<a/>").unwrap();
    let f = RestrictedFetcher::new(MockFetcher::new()).allow_dir(tmp.path());
    let path = tmp.path().join("a.xsd").display().to_string();
    f.fetch(&format!("FILE://{path}")).unwrap();
    assert_eq!(f.inner().calls(), vec![format!("file://{path}")]);
}

#[test]
fn wraps_file_fetcher() {
    let tmp = tempfile::tempdir().unwrap();
    let allowed = tmp.path().join("allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::write(allowed.join("a.xsd"), "<a/>").unwrap();
    std::fs::write(tmp.path().join("secret.xsd"), "<s/>").unwrap();

    let f = RestrictedFetcher::new(FileFetcher::new()).allow_dir(&allowed);
    let ok = f
        .fetch(&allowed.join("a.xsd").display().to_string())
        .unwrap();
    assert_eq!(ok.content, b"<a/>");
    let err = f
        .fetch(&tmp.path().join("secret.xsd").display().to_string())
        .unwrap_err();
    assert!(is_not_allowed(&err), "{err}");
}

#[test]
fn with_policy_uses_the_given_policy() {
    let policy = FetchPolicy::new().allow_url_prefix("https://example.com/");
    let f = RestrictedFetcher::with_policy(MockFetcher::new(), policy);
    assert!(f.fetch("https://example.com/a.xsd").is_ok());
    assert!(f.fetch("https://other.com/a.xsd").is_err());
    assert_eq!(
        f.policy().url_prefixes().collect::<Vec<_>>(),
        vec!["https://example.com/"]
    );
}

#[cfg(feature = "tokio")]
mod async_tests {
    use super::*;
    use crate::schema::fetcher::AsyncSchemaFetcher;

    #[async_trait::async_trait]
    impl AsyncSchemaFetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> Result<FetchResult> {
            Ok(self.respond(url))
        }
    }

    #[tokio::test]
    async fn async_allowed_passes_disallowed_never_reaches_inner() {
        let f = AsyncRestrictedFetcher::new(MockFetcher::new())
            .allow_url_prefix("https://example.com/s/");
        assert!(f.fetch("https://example.com/s/a.xsd").await.is_ok());
        let err = f.fetch("https://evil.com/a.xsd").await.unwrap_err();
        assert!(is_not_allowed(&err), "{err}");
        assert_eq!(f.inner().calls(), vec!["https://example.com/s/a.xsd"]);
    }

    #[tokio::test]
    async fn async_redirect_outside_policy_is_rejected() {
        let f = AsyncRestrictedFetcher::new(MockFetcher::redirecting_to("https://evil.com/x"))
            .allow_url_prefix("https://example.com/");
        let err = f.fetch("https://example.com/a.xsd").await.unwrap_err();
        assert!(is_not_allowed(&err), "{err}");
    }
}
