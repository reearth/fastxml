//! CLI tool for XML schema validation.
//!
//! Usage: fastxml-validate [OPTIONS] <FILES>...
//!
//! Run with: cargo run --features ureq --bin fastxml-validate -- <files>

#![allow(clippy::collapsible_if)]

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Parser;
use serde::Serialize;

use fastxml::error::StructuredError;
use fastxml::schema::{
    DefaultFetcher, FetchResult, InMemoryStore, SchemaFetcher, SchemaStore,
    streaming_validate_with_schema_location_and_fetcher,
};

/// XML Schema Validator CLI
#[derive(Parser, Debug)]
#[command(name = "fastxml-validate")]
#[command(author, version, about = "Validate XML files against XSD schemas", long_about = None)]
struct Args {
    /// XML files to validate (local paths or URLs)
    #[arg(required = true)]
    files: Vec<String>,

    /// Schema file path (default: auto-detect from xsi:schemaLocation)
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
            message: err.message.clone(),
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
    let store = Arc::new(InMemoryStore::new());
    let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

    for file_path in &args.files {
        let result = validate_file(
            file_path,
            args,
            Arc::clone(&store),
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

    // Exit code: 0 if all valid, 1 if any invalid
    if invalid_count > 0 { Ok(1) } else { Ok(0) }
}

fn validate_file(
    file_path: &str,
    args: &Args,
    store: Arc<InMemoryStore>,
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
        if args.schema.is_some() {
            println!("  Schema: {}", args.schema.as_ref().unwrap());
        } else {
            println!("  Schema: auto-detected from xsi:schemaLocation");
        }
    }

    // Create fetcher with shared store
    let is_http = file_path.starts_with("http://") || file_path.starts_with("https://");
    let base_dir = if !is_http {
        Path::new(file_path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    } else {
        std::env::current_dir().unwrap_or_default()
    };

    let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut fetcher = CachingFetcher::new(
        DefaultFetcher::with_base_dir(base_dir),
        Arc::clone(&store),
        Arc::clone(&downloaded_urls),
    );

    // For HTTP URLs, set the base URL for relative schema path resolution
    if is_http {
        fetcher = fetcher.with_base_url(file_path.to_string());
    }

    if !args.json && !args.quiet && args.verbose {
        println!("  Resolving schemas...");
    }

    // Perform validation
    let reader = BufReader::new(content.as_slice());
    let errors = streaming_validate_with_schema_location_and_fetcher(reader, fetcher)?;

    let elapsed = start.elapsed();
    let time_ms = elapsed.as_millis() as u64;
    let throughput = size_bytes as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64();

    // Collect downloaded schema URLs
    let schemas_downloaded: Vec<String> = downloaded_urls.lock().unwrap().clone();
    let schema_count = store.len();

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

/// A fetcher wrapper that tracks downloaded URLs and uses a shared store
struct CachingFetcher {
    inner: DefaultFetcher,
    store: Arc<InMemoryStore>,
    downloaded_urls: Arc<Mutex<Vec<String>>>,
    /// Base URL for resolving relative paths (used for HTTP XML sources)
    base_url: Option<String>,
}

impl CachingFetcher {
    fn new(
        inner: DefaultFetcher,
        store: Arc<InMemoryStore>,
        downloaded_urls: Arc<Mutex<Vec<String>>>,
    ) -> Self {
        Self {
            inner,
            store,
            downloaded_urls,
            base_url: None,
        }
    }

    /// Set base URL for resolving relative schema paths
    fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    /// Resolve a potentially relative URL against the base URL
    fn resolve_url(&self, url: &str) -> String {
        // If already absolute, return as-is
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
            return url.to_string();
        }

        // Try to resolve against base URL
        if let Some(base) = &self.base_url {
            if base.starts_with("http://") || base.starts_with("https://") {
                // Find the last slash in the path
                if let Some(last_slash) = base.rfind('/') {
                    let base_dir = &base[..=last_slash];
                    let combined = format!("{}{}", base_dir, url);
                    return normalize_url_path(&combined);
                }
            }
        }

        // Can't resolve, return original
        url.to_string()
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

impl SchemaFetcher for CachingFetcher {
    fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        // Resolve relative URLs
        let resolved_url = self.resolve_url(url);

        // Check cache first (only by resolved absolute URL)
        // We don't cache by relative URL since the same relative path resolves to
        // different absolute URLs for different base URLs (XML file locations)
        if let Ok(Some(content)) = self.store.get(&resolved_url) {
            return Ok(FetchResult {
                content,
                final_url: resolved_url,
                redirected: false,
            });
        }

        // Fetch from network
        let result = self.inner.fetch(&resolved_url)?;

        // Store in cache by absolute URLs only
        // Never cache by relative URL since the same relative path resolves to
        // different absolute URLs for different base URLs (XML file locations)
        let _ = self.store.put(&result.final_url, &result.content);
        if result.final_url != resolved_url {
            let _ = self.store.put(&resolved_url, &result.content);
        }

        // Track downloaded URL
        self.downloaded_urls
            .lock()
            .unwrap()
            .push(result.final_url.clone());

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
    fn test_caching_fetcher_resolve_url_with_different_base_urls() {
        // This test verifies that the same relative path resolves to different
        // absolute URLs when the base URL changes
        let store = Arc::new(InMemoryStore::new());
        let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

        // Create fetcher with first base URL
        let fetcher1 = CachingFetcher::new(
            DefaultFetcher::new(),
            Arc::clone(&store),
            Arc::clone(&downloaded_urls),
        )
        .with_base_url("https://example.com/dir1/file1.xml".to_string());

        // Create fetcher with second base URL
        let fetcher2 = CachingFetcher::new(
            DefaultFetcher::new(),
            Arc::clone(&store),
            Arc::clone(&downloaded_urls),
        )
        .with_base_url("https://example.com/dir2/file2.xml".to_string());

        // Same relative path should resolve to different absolute URLs
        let relative_path = "../schemas/test.xsd";
        let resolved1 = fetcher1.resolve_url(relative_path);
        let resolved2 = fetcher2.resolve_url(relative_path);

        assert_eq!(resolved1, "https://example.com/schemas/test.xsd");
        assert_eq!(resolved2, "https://example.com/schemas/test.xsd");

        // Different base directories
        let fetcher3 = CachingFetcher::new(
            DefaultFetcher::new(),
            Arc::clone(&store),
            Arc::clone(&downloaded_urls),
        )
        .with_base_url("https://other.com/project/data/file.xml".to_string());

        let resolved3 = fetcher3.resolve_url(relative_path);
        assert_eq!(resolved3, "https://other.com/project/schemas/test.xsd");
    }

    #[test]
    fn test_caching_fetcher_resolve_url_deep_relative_path() {
        let store = Arc::new(InMemoryStore::new());
        let downloaded_urls = Arc::new(Mutex::new(Vec::<String>::new()));

        let fetcher = CachingFetcher::new(
            DefaultFetcher::new(),
            Arc::clone(&store),
            Arc::clone(&downloaded_urls),
        )
        .with_base_url("https://example.com/assets/abc/project/udx/area/file.xml".to_string());

        // This mimics the PLATEAU schema path: ../../schemas/iur/urf/3.1/urbanFunction.xsd
        let resolved = fetcher.resolve_url("../../schemas/iur/urf/3.1/urbanFunction.xsd");
        assert_eq!(
            resolved,
            "https://example.com/assets/abc/project/schemas/iur/urf/3.1/urbanFunction.xsd"
        );
    }
}
