//! The allow-list deciding which schema locations may be fetched.

use std::path::{Path, PathBuf};

use url::Url;

use super::super::error::FetchError;

/// An allow-list of schema locations: remote URL prefixes and local
/// directories.
///
/// A location is allowed when
///
/// - it is an `http://` or `https://` URL whose scheme, host and port equal
///   those of an allowed prefix and whose path starts with the prefix's path
///   at a segment boundary (both URLs are normalized first, so `..` and
///   `%2e%2e` segments are resolved and `https://example.com.evil.com/` does
///   not match the prefix `https://example.com/`), or
/// - it is a local path or `file://` URL that, after canonicalization
///   (resolving `..` and symlinks), lies inside one of the allowed
///   directories. The file must exist.
///
/// Every other location is rejected, including URLs with any other scheme.
/// An empty policy rejects everything.
///
/// Relative local paths are resolved against the [`base_dir`](Self::base_dir)
/// if one is set, otherwise against the current working directory.
///
/// # Examples
///
/// ```
/// use fastxml::schema::fetcher::FetchPolicy;
///
/// let policy = FetchPolicy::new().allow_url_prefix("https://schemas.opengis.net/");
///
/// assert!(policy.is_allowed("https://schemas.opengis.net/gml/3.1.1/base/gml.xsd"));
/// assert!(!policy.is_allowed("https://schemas.opengis.net.evil.com/gml.xsd"));
/// assert!(!policy.is_allowed("https://example.com/gml.xsd"));
/// assert!(!policy.is_allowed("/etc/passwd"));
///
/// // Local files must lie inside an allowed directory.
/// let dir = std::env::temp_dir();
/// let policy = policy.allow_dir(&dir);
/// assert!(policy.is_allowed(&dir.display().to_string()));
/// assert!(!policy.is_allowed(&format!("{}/../../../etc/hosts", dir.display())));
/// ```
#[derive(Debug, Clone, Default)]
pub struct FetchPolicy {
    url_prefixes: Vec<UrlPrefix>,
    dirs: Vec<PathBuf>,
    base_dir: Option<PathBuf>,
}

/// A parsed allowed URL prefix. `None` when the prefix given to
/// [`FetchPolicy::allow_url_prefix`] was not a valid `http`/`https` URL; such
/// an entry matches nothing.
#[derive(Debug, Clone)]
struct UrlPrefix {
    raw: String,
    parsed: Option<Url>,
}

/// How a location is interpreted.
enum Location {
    Remote(Url),
    Local(PathBuf),
}

impl FetchPolicy {
    /// Creates an empty policy, which allows nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allows `http`/`https` URLs under `prefix`.
    ///
    /// The prefix must be an absolute `http://` or `https://` URL. A prefix
    /// whose path ends in `/` (or is empty) allows everything below that
    /// directory; otherwise it allows that exact path and everything below
    /// `<path>/`. A prefix that is not a valid `http`/`https` URL matches
    /// nothing.
    pub fn allow_url_prefix(mut self, prefix: &str) -> Self {
        let parsed = Url::parse(prefix)
            .ok()
            .filter(|u| is_http(u) && u.host_str().is_some());
        if parsed.is_none() {
            tracing::warn!(prefix, "ignoring invalid allowed URL prefix");
        }
        self.url_prefixes.push(UrlPrefix {
            raw: prefix.to_string(),
            parsed,
        });
        self
    }

    /// Allows local files inside `dir` (and its subdirectories).
    ///
    /// A relative `dir` is made absolute against the current working
    /// directory now; it is canonicalized when a location is checked.
    pub fn allow_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.dirs.push(absolute(dir.as_ref()));
        self
    }

    /// Sets the directory that relative local paths are resolved against
    /// (default: the current working directory).
    ///
    /// This only affects how relative locations are interpreted; the
    /// directory is not allowed by itself unless also passed to
    /// [`allow_dir`](Self::allow_dir).
    pub fn base_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.base_dir = Some(absolute(dir.as_ref()));
        self
    }

    /// Returns `true` if `location` is allowed.
    pub fn is_allowed(&self, location: &str) -> bool {
        self.check(location).is_ok()
    }

    /// Checks `location` against the policy.
    ///
    /// On success returns the location to hand to the underlying fetcher:
    /// the normalized URL for a remote location, the location unchanged for
    /// an absolute path or `file://` URL, and the absolute path (joined onto
    /// the base directory) for a relative path, so the fetcher reads exactly
    /// the file that was checked.
    ///
    /// # Errors
    ///
    /// Returns [`FetchError::NotAllowed`] naming the location and the reason.
    pub fn check(&self, location: &str) -> Result<String, FetchError> {
        let deny = |reason: String| FetchError::NotAllowed {
            url: location.to_string(),
            reason,
        };
        match self.classify(location).map_err(deny)? {
            Location::Remote(url) => {
                if self.url_allowed(&url) {
                    Ok(url.into())
                } else {
                    Err(deny("URL is not under any allowed URL prefix".to_string()))
                }
            }
            Location::Local(path) => {
                let canonical = path
                    .canonicalize()
                    .map_err(|e| deny(format!("cannot resolve {}: {}", path.display(), e)))?;
                if self.dir_allowed(&canonical) {
                    Ok(
                        if Path::new(location).is_relative() && !has_file_scheme(location) {
                            path.display().to_string()
                        } else {
                            location.to_string()
                        },
                    )
                } else {
                    Err(deny(format!(
                        "{} is not inside any allowed directory",
                        canonical.display()
                    )))
                }
            }
        }
    }

    fn classify(&self, location: &str) -> Result<Location, String> {
        if has_file_scheme(location) {
            // Interpreted the way `FileFetcher` reads it: everything after
            // `file://` is the path. Only local (empty-host) URLs qualify.
            let rest = &location["file://".len()..];
            let path = PathBuf::from(rest);
            if !path.is_absolute() {
                return Err("only file:// URLs with an absolute local path are supported".into());
            }
            return Ok(Location::Local(path));
        }
        match Url::parse(location) {
            Ok(url) if is_http(&url) => Ok(Location::Remote(url)),
            // A one-letter "scheme" is a Windows drive letter.
            Ok(url) if url.scheme().len() > 1 => {
                Err(format!("unsupported URL scheme `{}`", url.scheme()))
            }
            _ => {
                let path = Path::new(location);
                Ok(Location::Local(if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    match &self.base_dir {
                        Some(base) => base.join(path),
                        None => absolute(path),
                    }
                }))
            }
        }
    }

    fn url_allowed(&self, url: &Url) -> bool {
        // A server may decode an encoded separator and walk out of the prefix.
        let path = url.path().to_ascii_lowercase();
        if path.contains("%2f") || path.contains("%5c") {
            return false;
        }
        self.url_prefixes
            .iter()
            .filter_map(|p| p.parsed.as_ref())
            .any(|prefix| url_matches_prefix(url, prefix))
    }

    fn dir_allowed(&self, canonical: &Path) -> bool {
        self.dirs.iter().any(|dir| {
            dir.canonicalize()
                .map(|dir| canonical.starts_with(dir))
                .unwrap_or(false)
        })
    }

    /// The allowed URL prefixes, as given.
    pub fn url_prefixes(&self) -> impl Iterator<Item = &str> {
        self.url_prefixes.iter().map(|p| p.raw.as_str())
    }

    /// The allowed directories (absolute, not yet canonicalized).
    pub fn dirs(&self) -> impl Iterator<Item = &Path> {
        self.dirs.iter().map(PathBuf::as_path)
    }
}

fn is_http(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
}

fn has_file_scheme(location: &str) -> bool {
    location
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("file://"))
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn url_matches_prefix(url: &Url, prefix: &Url) -> bool {
    if url.scheme() != prefix.scheme()
        || url.host() != prefix.host()
        || url.port_or_known_default() != prefix.port_or_known_default()
    {
        return false;
    }
    let (path, prefix_path) = (url.path(), prefix.path());
    if prefix_path.ends_with('/') {
        return path.starts_with(prefix_path);
    }
    path == prefix_path
        || path
            .strip_prefix(prefix_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opengis() -> FetchPolicy {
        FetchPolicy::new().allow_url_prefix("https://schemas.opengis.net/gml/")
    }

    #[test]
    fn empty_policy_allows_nothing() {
        let p = FetchPolicy::new();
        assert!(!p.is_allowed("https://schemas.opengis.net/gml/a.xsd"));
        assert!(!p.is_allowed("/etc/passwd"));
    }

    #[test]
    fn url_under_prefix_is_allowed() {
        let p = opengis();
        assert!(p.is_allowed("https://schemas.opengis.net/gml/3.1.1/base/gml.xsd"));
        assert!(p.is_allowed("https://SCHEMAS.opengis.net:443/gml/a.xsd"));
        assert!(p.is_allowed("https://schemas.opengis.net/gml/a.xsd?x=1#f"));
    }

    #[test]
    fn host_suffix_and_userinfo_tricks_are_rejected() {
        let p = opengis();
        assert!(!p.is_allowed("https://schemas.opengis.net.evil.com/gml/a.xsd"));
        assert!(!p.is_allowed("https://evil.com/schemas.opengis.net/gml/a.xsd"));
        assert!(!p.is_allowed("https://schemas.opengis.net@evil.com/gml/a.xsd"));
        assert!(!p.is_allowed("https://evilschemas.opengis.net/gml/a.xsd"));
    }

    #[test]
    fn scheme_and_port_must_match() {
        let p = opengis();
        assert!(!p.is_allowed("http://schemas.opengis.net/gml/a.xsd"));
        assert!(!p.is_allowed("https://schemas.opengis.net:8443/gml/a.xsd"));
    }

    #[test]
    fn dot_segments_cannot_escape_the_prefix() {
        let p = opengis();
        assert!(!p.is_allowed("https://schemas.opengis.net/gml/../citygml/a.xsd"));
        assert!(!p.is_allowed("https://schemas.opengis.net/gml/%2e%2e/citygml/a.xsd"));
        assert!(!p.is_allowed("https://schemas.opengis.net/gml/..%2fcitygml/a.xsd"));
        assert!(p.is_allowed("https://schemas.opengis.net/gml/x/../a.xsd"));
        assert!(p.is_allowed("https://schemas.opengis.net/citygml/../gml/a.xsd"));
    }

    #[test]
    fn prefix_without_trailing_slash_matches_on_segment_boundary() {
        let p = FetchPolicy::new().allow_url_prefix("https://example.com/schemas");
        assert!(p.is_allowed("https://example.com/schemas"));
        assert!(p.is_allowed("https://example.com/schemas/a.xsd"));
        assert!(!p.is_allowed("https://example.com/schemas2/a.xsd"));
        assert!(!p.is_allowed("https://example.com/schemasfoo"));
    }

    #[test]
    fn host_only_prefix_allows_whole_host() {
        let p = FetchPolicy::new().allow_url_prefix("https://example.com");
        assert!(p.is_allowed("https://example.com/a/b.xsd"));
        assert!(!p.is_allowed("https://example.com.evil.com/a/b.xsd"));
    }

    #[test]
    fn invalid_prefix_matches_nothing() {
        let p = FetchPolicy::new()
            .allow_url_prefix("not a url")
            .allow_url_prefix("ftp://example.com/");
        assert!(!p.is_allowed("ftp://example.com/a.xsd"));
        assert!(!p.is_allowed("https://example.com/a.xsd"));
    }

    #[test]
    fn check_returns_normalized_url() {
        let p = opengis();
        assert_eq!(
            p.check("https://SCHEMAS.opengis.net/gml/x/../a.xsd")
                .unwrap(),
            "https://schemas.opengis.net/gml/a.xsd"
        );
    }

    #[test]
    fn other_schemes_are_rejected() {
        let p = opengis().allow_dir("/");
        assert!(!p.is_allowed("ftp://schemas.opengis.net/gml/a.xsd"));
        assert!(!p.is_allowed("data:text/plain,hello"));
    }

    #[test]
    fn rejection_names_the_location() {
        let err = opengis().check("https://evil.com/a.xsd").unwrap_err();
        assert!(matches!(err, FetchError::NotAllowed { .. }));
        assert!(err.to_string().contains("https://evil.com/a.xsd"), "{err}");
    }

    /// `<tmp>/allowed/{a.xsd,sub/b.xsd}` and `<tmp>/outside/secret.xsd`.
    fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = tmp.path().join("allowed");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(allowed.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(allowed.join("a.xsd"), "<a/>").unwrap();
        std::fs::write(allowed.join("sub/b.xsd"), "<b/>").unwrap();
        std::fs::write(outside.join("secret.xsd"), "<s/>").unwrap();
        (tmp, allowed, outside)
    }

    #[test]
    fn local_file_inside_allowed_dir_is_allowed() {
        let (_tmp, allowed, _) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed);
        let a = allowed.join("a.xsd").display().to_string();
        assert_eq!(p.check(&a).unwrap(), a);
        assert!(p.is_allowed(&allowed.join("sub/b.xsd").display().to_string()));
    }

    #[test]
    fn local_file_outside_allowed_dir_is_rejected() {
        let (_tmp, allowed, outside) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed);
        assert!(!p.is_allowed(&outside.join("secret.xsd").display().to_string()));
        assert!(!p.is_allowed("/etc/hosts"));
    }

    #[test]
    fn sibling_dir_with_common_name_prefix_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = tmp.path().join("schemas");
        let sibling = tmp.path().join("schemas2");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("x.xsd"), "<x/>").unwrap();
        let p = FetchPolicy::new().allow_dir(&allowed);
        assert!(!p.is_allowed(&sibling.join("x.xsd").display().to_string()));
    }

    #[test]
    fn dot_dot_traversal_out_of_allowed_dir_is_rejected() {
        let (_tmp, allowed, _) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed);
        let escape = format!("{}/sub/../../outside/secret.xsd", allowed.display());
        assert!(!p.is_allowed(&escape));
        let stays = format!("{}/sub/../a.xsd", allowed.display());
        assert!(p.is_allowed(&stays));
    }

    #[test]
    fn missing_local_file_is_rejected() {
        let (_tmp, allowed, _) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed);
        assert!(!p.is_allowed(&allowed.join("nope.xsd").display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_allowed_dir_is_rejected() {
        let (_tmp, allowed, outside) = dirs();
        std::os::unix::fs::symlink(outside.join("secret.xsd"), allowed.join("link.xsd")).unwrap();
        std::os::unix::fs::symlink(&outside, allowed.join("linkdir")).unwrap();
        let p = FetchPolicy::new().allow_dir(&allowed);
        assert!(!p.is_allowed(&allowed.join("link.xsd").display().to_string()));
        assert!(!p.is_allowed(&allowed.join("linkdir/secret.xsd").display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn allowed_dir_given_through_a_symlink_still_matches() {
        let (tmp, allowed, _) = dirs();
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&allowed, &alias).unwrap();
        let p = FetchPolicy::new().allow_dir(&alias);
        assert!(p.is_allowed(&allowed.join("a.xsd").display().to_string()));
        assert!(p.is_allowed(&alias.join("a.xsd").display().to_string()));
    }

    #[test]
    fn file_urls_are_checked_like_paths() {
        let (_tmp, allowed, outside) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed);
        let ok = format!("file://{}", allowed.join("a.xsd").display());
        assert_eq!(p.check(&ok).unwrap(), ok);
        assert!(!p.is_allowed(&format!("file://{}", outside.join("secret.xsd").display())));
        assert!(!p.is_allowed(&format!(
            "file://{}/../outside/secret.xsd",
            allowed.display()
        )));
    }

    #[test]
    fn file_urls_with_a_host_are_rejected() {
        let (_tmp, allowed, _) = dirs();
        let p = FetchPolicy::new().allow_dir("/");
        let with_host = format!("file://evil.com{}", allowed.join("a.xsd").display());
        assert!(!p.is_allowed(&with_host));
    }

    #[test]
    fn relative_paths_resolve_against_base_dir() {
        let (_tmp, allowed, outside) = dirs();
        let p = FetchPolicy::new().allow_dir(&allowed).base_dir(&allowed);
        assert_eq!(
            p.check("sub/b.xsd").unwrap(),
            allowed.join("sub/b.xsd").display().to_string()
        );
        assert!(!p.is_allowed("../outside/secret.xsd"));

        // The base directory itself is not allowed implicitly.
        let p = FetchPolicy::new().allow_dir(&allowed).base_dir(&outside);
        assert!(!p.is_allowed("secret.xsd"));
    }
}
