//! CLI tool for XML schema validation.
//!
//! Usage: `fastxml-validate [OPTIONS] <FILES>...`
//!
//! Run with: `cargo run --features ureq --bin fastxml-validate -- <files>`

#![allow(clippy::collapsible_if)]

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Parser;
use serde::Serialize;

use fastxml::error::StructuredError;
use fastxml::schema::{DefaultFetcher, FetchResult, Schema, SchemaFetcher, Validator};

const AFTER_HELP: &str = "\
Schemas:
  Without --schema, each file is validated against the schemas named by its
  root element's xsi:schemaLocation / xsi:noNamespaceSchemaLocation
  attributes. Relative locations are resolved against the file's own
  directory (or URL). A file whose schema cannot be fetched, parsed or
  compiled, or that names no schema at all, is reported invalid with an
  error saying why.

Exit codes:
  0  every file is valid
  1  at least one file is invalid (including files whose schema could not
     be loaded)
  2  the run could not complete: bad arguments, an unreadable or malformed
     input file, or a --schema that could not be loaded";

/// XML Schema Validator CLI
#[derive(Parser, Debug)]
#[command(name = "fastxml-validate")]
#[command(author, version, about = "Validate XML files against XSD schemas", long_about = None)]
#[command(after_help = AFTER_HELP)]
struct Args {
    /// XML files to validate (local paths or URLs)
    #[arg(required = true)]
    files: Vec<String>,

    /// Validate every file against this schema (path or URL; its imports and
    /// includes are fetched relative to it) instead of the schemas named by
    /// each file's xsi:schemaLocation
    #[arg(short, long, value_name = "PATH")]
    schema: Option<String>,

    /// Output as JSON
    #[arg(short, long)]
    json: bool,

    /// Only show errors (no timing/memory info)
    #[arg(short, long)]
    quiet: bool,

    /// Show detailed validation progress
    #[arg(short, long)]
    verbose: bool,

    /// Show timing and memory statistics
    #[arg(long)]
    stats: bool,
}

/// Result for a single file validation
#[derive(Debug, Serialize)]
struct FileResult {
    path: String,
    size_bytes: u64,
    valid: bool,
    errors: Vec<ErrorInfo>,
    schemas_downloaded: Vec<String>,
    time_ms: u64,
    throughput_mb_s: f64,
}

/// Error information for JSON output
#[derive(Debug, Serialize)]
struct ErrorInfo {
    line: Option<usize>,
    column: Option<usize>,
    level: String,
    message: String,
}

/// Summary of all validations
#[derive(Debug, Serialize)]
struct Summary {
    total: usize,
    valid: usize,
    invalid: usize,
}

/// Full JSON output
#[derive(Debug, Serialize)]
struct JsonOutput {
    files: Vec<FileResult>,
    summary: Summary,
}

impl From<&StructuredError> for ErrorInfo {
    fn from(err: &StructuredError) -> Self {
        ErrorInfo {
            line: err.line(),
            column: err.column(),
            level: err.level.to_string(),
            message: err.message.to_string(),
        }
    }
}

fn main() {
    let args = Args::parse();

    match run(&args) {
        Ok(exit_code) => std::process::exit(exit_code),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        }
    }
}

fn run(args: &Args) -> Result<i32, Box<dyn std::error::Error>> {
    let mut results = Vec::new();
    let cache = Arc::new(DefaultFetcher::new());
    let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

    let schema = match &args.schema {
        Some(location) => {
            Some(Arc::new(load_schema(location, &cache).map_err(|e| {
                format!("could not load --schema {}: {}", location, e)
            })?))
        }
        None => None,
    };

    for file_path in &args.files {
        let result = validate_file(
            file_path,
            args,
            schema.clone(),
            Arc::clone(&cache),
            Arc::clone(&downloaded_urls),
        )?;
        results.push(result);
    }

    let valid_count = results.iter().filter(|r| r.valid).count();
    let invalid_count = results.len() - valid_count;

    if args.json {
        let output = JsonOutput {
            files: results,
            summary: Summary {
                total: args.files.len(),
                valid: valid_count,
                invalid: invalid_count,
            },
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Print summary for multiple files
        if args.files.len() > 1 && !args.quiet {
            println!();
            println!(
                "Summary: {} files, {} valid, {} invalid",
                args.files.len(),
                valid_count,
                invalid_count
            );
        }
    }

    // Exit code: 0 if all valid, 1 if any invalid (see AFTER_HELP)
    if invalid_count > 0 { Ok(1) } else { Ok(0) }
}

/// Compiles the `--schema` document, resolving its imports/includes relative
/// to its own location.
fn load_schema(location: &str, fetcher: &DefaultFetcher) -> fastxml::error::Result<Schema> {
    let location = if is_url(location) {
        location.to_string()
    } else {
        std::path::absolute(location)?.display().to_string()
    };
    let fetched = fetcher.fetch(&location)?;
    Schema::builder()
        .add(fetched.final_url, fetched.content)
        .resolve_with(fetcher)
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://")
}

fn validate_file(
    file_path: &str,
    args: &Args,
    schema: Option<Arc<Schema>>,
    cache: Arc<DefaultFetcher>,
    global_downloaded_urls: Arc<Mutex<Vec<String>>>,
) -> Result<FileResult, Box<dyn std::error::Error>> {
    let start = Instant::now();

    // Determine if URL or local file
    let (content, size_bytes) =
        if file_path.starts_with("http://") || file_path.starts_with("https://") {
            fetch_url(file_path, args)?
        } else {
            read_local_file(file_path, args)?
        };

    if !args.json && !args.quiet {
        println!(
            "Validating: {} ({:.2} MB)",
            file_path,
            size_bytes as f64 / 1024.0 / 1024.0
        );
        if let Some(schema) = &args.schema {
            println!("  Schema: {}", schema);
        } else {
            println!("  Schema: auto-detected from xsi:schemaLocation");
        }
    }

    // Create fetcher with shared cache
    let is_http = file_path.starts_with("http://") || file_path.starts_with("https://");

    let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));
    // Relative schema locations are resolved against the document itself:
    // its URL, or the directory containing the local file.
    let base = if is_http {
        Base::Url(file_path.to_string())
    } else {
        let dir = std::path::absolute(file_path)?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        Base::Dir(dir)
    };
    let fetcher = UrlTrackingFetcher::new(Arc::clone(&cache), Arc::clone(&downloaded_urls), base);

    if !args.json && !args.quiet && args.verbose && schema.is_none() {
        println!("  Resolving schemas...");
    }

    // Perform validation
    let reader = BufReader::new(content.as_slice());
    let validator = Validator::from_reader(reader);
    let errors = match schema {
        Some(schema) => validator.schema(schema).run()?,
        None => validator.run_with(fetcher)?,
    }
    .into_entries();

    let elapsed = start.elapsed();
    let time_ms = elapsed.as_millis() as u64;
    let throughput = size_bytes as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64();

    // Collect downloaded schema URLs
    let schemas_downloaded: Vec<String> = downloaded_urls.lock().unwrap().clone();
    let schema_count = cache.len();

    // Add to global downloaded URLs
    global_downloaded_urls
        .lock()
        .unwrap()
        .extend(schemas_downloaded.clone());

    // Separate errors and warnings
    let error_count = errors.iter().filter(|e| e.is_error()).count();
    let warning_count = errors.len() - error_count;
    let valid = error_count == 0;

    if !args.json {
        if !args.quiet && args.verbose {
            let cache_status = if schemas_downloaded.is_empty() {
                "using cached schemas".to_string()
            } else {
                format!("{} schemas", schema_count)
            };
            println!("  Resolving schemas... done ({})", cache_status);

            if !schemas_downloaded.is_empty() {
                println!("  Downloaded schemas:");
                for url in &schemas_downloaded {
                    println!("    - {}", url);
                }
            }
        }

        if !args.quiet && args.verbose {
            println!("  Validating... done");
        }

        // Print errors
        if !errors.is_empty() {
            println!();
            println!("  Errors: {}", error_count);
            for err in &errors {
                if err.is_error() {
                    if let Some(line) = err.line() {
                        print!("    line {}", line);
                        if let Some(col) = err.column() {
                            print!(":{}", col);
                        }
                        print!(": ");
                    } else {
                        print!("    ");
                    }
                    println!("{}", err.message);
                }
            }

            if warning_count > 0 && args.verbose {
                println!();
                println!("  Warnings: {}", warning_count);
                for err in &errors {
                    if !err.is_error() {
                        if let Some(line) = err.line() {
                            print!("    line {}: ", line);
                        } else {
                            print!("    ");
                        }
                        println!("{}", err.message);
                    }
                }
            }
        } else if !args.quiet {
            println!();
            println!("  No errors found");
        }

        // Print timing info
        if !args.quiet && (args.stats || args.verbose) {
            println!();
            println!("  Time: {}ms ({:.2} MB/s)", time_ms, throughput);
        }

        println!();
    }

    Ok(FileResult {
        path: file_path.to_string(),
        size_bytes,
        valid,
        errors: errors.iter().map(ErrorInfo::from).collect(),
        schemas_downloaded,
        time_ms,
        throughput_mb_s: throughput,
    })
}

fn read_local_file(
    file_path: &str,
    args: &Args,
) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path).into());
    }

    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let compressed_size = metadata.len();

    let mut content = Vec::new();

    // Check if gzip compressed
    if file_path.ends_with(".gz") {
        use flate2::read::GzDecoder;
        let mut decoder = GzDecoder::new(BufReader::new(file));
        decoder.read_to_end(&mut content)?;

        if args.verbose && !args.json && !args.quiet {
            println!(
                "  Decompressed: {:.2} MB -> {:.2} MB",
                compressed_size as f64 / 1024.0 / 1024.0,
                content.len() as f64 / 1024.0 / 1024.0
            );
        }
    } else {
        BufReader::new(file).read_to_end(&mut content)?;
    }

    let size = content.len() as u64;
    Ok((content, size))
}

fn fetch_url(url: &str, args: &Args) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
    if args.verbose && !args.json && !args.quiet {
        println!("  Downloading: {}", url);
    }

    let response = ureq::get(url)
        .set("Accept-Encoding", "gzip")
        .call()
        .map_err(|e| format!("Failed to fetch URL {}: {}", url, e))?;

    let content_encoding = response.header("Content-Encoding").map(|s| s.to_string());
    let mut content = Vec::new();
    response.into_reader().read_to_end(&mut content)?;

    // Handle gzip decompression if needed
    let is_gzip = content_encoding
        .as_ref()
        .map(|s| s.contains("gzip"))
        .unwrap_or(false)
        || url.ends_with(".gz");

    let final_content = if is_gzip {
        use flate2::read::GzDecoder;
        let mut decoder = GzDecoder::new(content.as_slice());
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;

        if args.verbose && !args.json && !args.quiet {
            println!(
                "  Decompressed: {:.2} MB -> {:.2} MB",
                content.len() as f64 / 1024.0 / 1024.0,
                decompressed.len() as f64 / 1024.0 / 1024.0
            );
        }
        decompressed
    } else {
        content
    };

    let size = final_content.len() as u64;
    Ok((final_content, size))
}

/// What a document's relative schema locations are resolved against.
enum Base {
    /// The URL of a remote XML document.
    Url(String),
    /// The (absolute) directory containing a local XML document.
    Dir(PathBuf),
}

/// A fetcher wrapper that tracks downloaded URLs and delegates to a shared DefaultFetcher.
///
/// This struct adds URL tracking (recording which URLs were freshly downloaded) and
/// resolution of relative schema locations against the XML document's own
/// location on top of `DefaultFetcher`, which handles the actual caching.
struct UrlTrackingFetcher {
    inner: Arc<DefaultFetcher>,
    downloaded_urls: Arc<Mutex<Vec<String>>>,
    /// Base for resolving relative locations named by the document
    base: Base,
}

impl UrlTrackingFetcher {
    fn new(
        inner: Arc<DefaultFetcher>,
        downloaded_urls: Arc<Mutex<Vec<String>>>,
        base: Base,
    ) -> Self {
        Self {
            inner,
            downloaded_urls,
            base,
        }
    }

    /// Resolve a potentially relative location against the document's base
    fn resolve_url(&self, url: &str) -> String {
        // If already absolute, return as-is
        if is_url(url) || Path::new(url).is_absolute() {
            return url.to_string();
        }

        match &self.base {
            Base::Url(base) => {
                // Find the last slash in the path
                if let Some(last_slash) = base.rfind('/') {
                    let base_dir = &base[..=last_slash];
                    let combined = format!("{}{}", base_dir, url);
                    return normalize_url_path(&combined);
                }
                url.to_string()
            }
            Base::Dir(dir) => dir.join(url).display().to_string(),
        }
    }
}

/// Normalizes a URL path by resolving `.` and `..` components.
fn normalize_url_path(url: &str) -> String {
    // Split URL into protocol+host and path
    let (prefix, path) = if let Some(pos) = url.find("://") {
        let after_protocol = &url[pos + 3..];
        if let Some(slash_pos) = after_protocol.find('/') {
            let host_end = pos + 3 + slash_pos;
            (&url[..host_end], &url[host_end..])
        } else {
            return url.to_string();
        }
    } else {
        return url.to_string();
    };

    // Normalize the path
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }

    format!("{}/{}", prefix, segments.join("/"))
}

impl SchemaFetcher for UrlTrackingFetcher {
    fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        // Resolve relative URLs
        let resolved_url = self.resolve_url(url);

        // Track cache size before fetch to detect new downloads
        let cache_size_before = self.inner.len();

        // Delegate to the shared DefaultFetcher (handles caching + actual fetching)
        let result = self.inner.fetch(&resolved_url)?;

        // If the cache grew, this was a fresh download — track the URL
        if self.inner.len() > cache_size_before {
            self.downloaded_urls
                .lock()
                .unwrap()
                .push(result.final_url.clone());
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url_path() {
        // Basic normalization
        assert_eq!(
            normalize_url_path("https://example.com/a/b/c"),
            "https://example.com/a/b/c"
        );

        // With parent directory references
        assert_eq!(
            normalize_url_path("https://example.com/a/b/../c"),
            "https://example.com/a/c"
        );

        // Multiple parent references
        assert_eq!(
            normalize_url_path("https://example.com/a/b/c/../../d"),
            "https://example.com/a/d"
        );

        // Current directory references
        assert_eq!(
            normalize_url_path("https://example.com/a/./b/./c"),
            "https://example.com/a/b/c"
        );
    }

    #[test]
    fn test_url_tracking_fetcher_resolve_url_with_different_base_urls() {
        // This test verifies that the same relative path resolves to different
        // absolute URLs when the base URL changes
        let cache = Arc::new(DefaultFetcher::new());
        let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

        // Create fetcher with first base URL
        let fetcher1 = UrlTrackingFetcher::new(
            Arc::clone(&cache),
            Arc::clone(&downloaded_urls),
            Base::Url("https://example.com/dir1/file1.xml".to_string()),
        );

        // Create fetcher with second base URL
        let fetcher2 = UrlTrackingFetcher::new(
            Arc::clone(&cache),
            Arc::clone(&downloaded_urls),
            Base::Url("https://example.com/dir2/file2.xml".to_string()),
        );

        // Same relative path should resolve to different absolute URLs
        let relative_path = "../schemas/test.xsd";
        let resolved1 = fetcher1.resolve_url(relative_path);
        let resolved2 = fetcher2.resolve_url(relative_path);

        assert_eq!(resolved1, "https://example.com/schemas/test.xsd");
        assert_eq!(resolved2, "https://example.com/schemas/test.xsd");

        // Different base directories
        let fetcher3 = UrlTrackingFetcher::new(
            Arc::clone(&cache),
            Arc::clone(&downloaded_urls),
            Base::Url("https://other.com/project/data/file.xml".to_string()),
        );

        let resolved3 = fetcher3.resolve_url(relative_path);
        assert_eq!(resolved3, "https://other.com/project/schemas/test.xsd");
    }

    #[test]
    fn test_url_tracking_fetcher_resolve_url_deep_relative_path() {
        let cache = Arc::new(DefaultFetcher::new());
        let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

        let fetcher = UrlTrackingFetcher::new(
            Arc::clone(&cache),
            Arc::clone(&downloaded_urls),
            Base::Url("https://example.com/assets/abc/project/udx/area/file.xml".to_string()),
        );

        // This mimics the PLATEAU schema path: ../../schemas/iur/urf/3.1/urbanFunction.xsd
        let resolved = fetcher.resolve_url("../../schemas/iur/urf/3.1/urbanFunction.xsd");
        assert_eq!(
            resolved,
            "https://example.com/assets/abc/project/schemas/iur/urf/3.1/urbanFunction.xsd"
        );
    }
}
