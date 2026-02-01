//! XML Benchmark - Performance testing for DOM and Streaming parsing with schema validation.
//!
//! This benchmark measures:
//! - Parse throughput (MB/s)
//! - Validation throughput (MB/s)
//! - Peak memory usage
//! - Schema cache statistics
//!
//! # Usage
//!
//! ```bash
//! # Pass URLs or file paths directly as arguments
//! cargo run --release --bench xml_benchmark --features sync -- \
//!     https://example.com/file1.xml ./local_file.xml
//!
//! # Read inputs from stdin
//! echo "https://example.com/file.xml" | cargo run --release --bench xml_benchmark --features sync
//!
//! # Read inputs from a file via stdin
//! cat inputs.txt | cargo run --release --bench xml_benchmark --features sync
//!
//! # With options
//! cargo run --release --bench xml_benchmark --features sync -- \
//!     --cache-dir ./my_cache --streaming-only \
//!     ./large_file.xml
//! ```

use std::fs;
use std::io::{BufRead, BufReader, IsTerminal, Read, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::schema::store::SchemaStore;
use fastxml::schema::tempdir::TempDirStore;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::validator::{StreamingSchemaValidator, XmlSchemaValidationContext};
use fastxml::schema::xsd::create_builtin_schema;
use fastxml::{parse, parse_schema_locations, validate_document_by_schema_context};

// =============================================================================
// CLI Configuration
// =============================================================================

struct Config {
    urls: Vec<String>,
    cache_dir: PathBuf,
    streaming_only: bool,
    dom_only: bool,
}

impl Config {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();

        let mut cache_dir = PathBuf::from("benches/cache");
        let mut streaming_only = false;
        let mut dom_only = false;
        let mut urls = Vec::new();
        let mut show_help = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => show_help = true,
                "--cache-dir" => {
                    i += 1;
                    if i < args.len() {
                        cache_dir = PathBuf::from(&args[i]);
                    }
                }
                "--streaming-only" => streaming_only = true,
                "--dom-only" => dom_only = true,
                arg if arg.starts_with("http://") || arg.starts_with("https://") => {
                    urls.push(arg.to_string());
                }
                arg if !arg.starts_with('-') => {
                    // Treat as URL even without http prefix
                    urls.push(arg.to_string());
                }
                _ => {
                    eprintln!("Unknown option: {}", args[i]);
                    std::process::exit(1);
                }
            }
            i += 1;
        }

        // Show help if requested
        if show_help {
            eprintln!("Usage: {} [options] [inputs...]", args[0]);
            eprintln!();
            eprintln!("XML Benchmark - Performance testing for DOM and Streaming parsing");
            eprintln!();
            eprintln!("Arguments:");
            eprintln!(
                "  [inputs...]         URLs or file paths to benchmark (can also be provided via stdin)"
            );
            eprintln!();
            eprintln!("Options:");
            eprintln!(
                "  --cache-dir <dir>   Directory for caching downloaded URLs (default: benches/cache)"
            );
            eprintln!("  --streaming-only    Run only streaming benchmarks");
            eprintln!("  --dom-only          Run only DOM benchmarks");
            eprintln!("  -h, --help          Show this help message");
            eprintln!();
            eprintln!("Examples:");
            eprintln!("  # Pass URLs or file paths as arguments");
            eprintln!("  {} https://example.com/file.xml ./local.xml", args[0]);
            eprintln!();
            eprintln!("  # Read inputs from stdin");
            eprintln!("  echo './my_file.xml' | {}", args[0]);
            eprintln!("  cat inputs.txt | {}", args[0]);
            std::process::exit(0);
        }

        // Read URLs from stdin if no URLs provided and stdin is not a terminal
        if urls.is_empty() && !std::io::stdin().is_terminal() {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().map_while(|l| l.ok()) {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    urls.push(line.to_string());
                }
            }
        }

        if urls.is_empty() {
            eprintln!("Error: No URLs provided. Use --help for usage information.");
            std::process::exit(1);
        }

        Self {
            urls,
            cache_dir,
            streaming_only,
            dom_only,
        }
    }
}

// =============================================================================
// Memory Tracking
// =============================================================================

#[cfg(feature = "profile")]
fn get_memory_usage() -> Option<usize> {
    memory_stats::memory_stats().map(|s| s.physical_mem)
}

#[cfg(not(feature = "profile"))]
fn get_memory_usage() -> Option<usize> {
    None
}

struct MemoryTracker {
    peak: AtomicUsize,
    initial: usize,
}

impl MemoryTracker {
    fn new() -> Self {
        let initial = get_memory_usage().unwrap_or(0);
        Self {
            peak: AtomicUsize::new(initial),
            initial,
        }
    }

    fn update(&self) {
        if let Some(current) = get_memory_usage() {
            self.peak.fetch_max(current, Ordering::Relaxed);
        }
    }

    fn peak_usage(&self) -> usize {
        self.peak
            .load(Ordering::Relaxed)
            .saturating_sub(self.initial)
    }
}

// =============================================================================
// Schema Management
// =============================================================================

struct SchemaManager {
    store: TempDirStore,
    compiled: CompiledSchema,
}

impl SchemaManager {
    fn new() -> Self {
        Self {
            store: TempDirStore::with_prefix("fastxml-bench-")
                .expect("Failed to create temp dir for schema cache"),
            compiled: create_builtin_schema(),
        }
    }

    fn cache_path(&self) -> &std::path::Path {
        self.store.path()
    }

    fn fetch_and_cache_schemas(&mut self, schema_locations: &[(String, String)]) {
        for (_namespace, location) in schema_locations {
            if self.store.contains(location) {
                continue;
            }

            if let Ok(content) = fetch_url(location) {
                let _ = self.store.put(location, &content);
                println!("    Cached schema: {} ({} bytes)", location, content.len());
            }
        }
    }

    fn total_cache_size(&self) -> usize {
        self.store.total_size()
    }

    fn schema_count(&self) -> usize {
        self.store.len()
    }

    fn get_compiled_schema(&self) -> Arc<CompiledSchema> {
        Arc::new(self.compiled.clone())
    }

    fn get_validation_context(&self) -> XmlSchemaValidationContext {
        XmlSchemaValidationContext::new(self.compiled.clone())
    }
}

fn fetch_url(url: &str) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    println!("    Downloading: {}", url);
    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(30))
        .call()?;

    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes)?;

    Ok(bytes)
}

// =============================================================================
// Benchmark Results
// =============================================================================

#[derive(Debug, Clone)]
struct DomBenchResult {
    file_name: String,
    file_size: usize,
    parse_time: Duration,
    validation_time: Duration,
    node_count: usize,
    peak_memory: usize,
    schema_cache_size: usize,
    schema_cache_count: usize,
}

impl DomBenchResult {
    fn throughput_mbps(&self) -> f64 {
        let secs = self.parse_time.as_secs_f64();
        if secs > 0.0 {
            (self.file_size as f64) / secs / 1_000_000.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
struct StreamBenchResult {
    file_name: String,
    file_size: usize,
    parse_time: Duration,
    validation_time: Duration,
    peak_memory: usize,
    schema_cache_size: usize,
    schema_cache_count: usize,
}

impl StreamBenchResult {
    fn throughput_mbps(&self) -> f64 {
        let secs = self.parse_time.as_secs_f64();
        if secs > 0.0 {
            (self.file_size as f64) / secs / 1_000_000.0
        } else {
            0.0
        }
    }

    fn validation_throughput_mbps(&self) -> f64 {
        let secs = self.validation_time.as_secs_f64();
        if secs > 0.0 {
            (self.file_size as f64) / secs / 1_000_000.0
        } else {
            0.0
        }
    }
}

// =============================================================================
// Streaming Handler
// =============================================================================

struct CountingHandler {
    element_count: usize,
    memory_tracker: Arc<MemoryTracker>,
}

impl CountingHandler {
    fn new(memory_tracker: Arc<MemoryTracker>) -> Self {
        Self {
            element_count: 0,
            memory_tracker,
        }
    }
}

impl XmlEventHandler for CountingHandler {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        if let XmlEvent::StartElement { .. } = event {
            self.element_count += 1;
            if self.element_count.is_multiple_of(10000) {
                self.memory_tracker.update();
            }
        }
        Ok(())
    }
}

// =============================================================================
// File Management
// =============================================================================

fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

fn get_display_name(input: &str) -> &str {
    if is_url(input) {
        input.split('/').next_back().unwrap_or("unknown.xml")
    } else {
        Path::new(input)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown.xml")
    }
}

fn url_to_cache_path(url: &str, cache_dir: &Path) -> PathBuf {
    let file_name = url.split('/').next_back().unwrap_or("unknown.xml");
    cache_dir.join(file_name)
}

fn load_file(
    input: &str,
    cache_dir: &Path,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    if is_url(input) {
        // URL: download with caching
        let cache_path = url_to_cache_path(input, cache_dir);

        if cache_path.exists() {
            return Ok(fs::read(&cache_path)?);
        }

        println!("  Downloading: {}", input);
        let response = ureq::get(input).call()?;

        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes)?;

        fs::create_dir_all(cache_dir)?;
        let mut file = fs::File::create(&cache_path)?;
        file.write_all(&bytes)?;

        Ok(bytes)
    } else {
        // Local file path
        Ok(fs::read(input)?)
    }
}

// =============================================================================
// DOM Benchmark
// =============================================================================

fn bench_dom(input: &str, content: &[u8], schema_manager: &mut SchemaManager) -> DomBenchResult {
    let file_name = get_display_name(input).to_string();
    let file_size = content.len();

    let memory_tracker = Arc::new(MemoryTracker::new());

    // Parse
    let parse_start = Instant::now();
    let doc = parse(content).expect("Failed to parse XML");
    let parse_time = parse_start.elapsed();
    memory_tracker.update();

    // Extract and cache schemas
    if let Ok(schema_locations) = parse_schema_locations(&doc)
        && !schema_locations.is_empty()
    {
        println!("  Found {} schema locations", schema_locations.len());
        schema_manager.fetch_and_cache_schemas(&schema_locations);
    }

    // Validate
    let validation_start = Instant::now();
    let ctx = schema_manager.get_validation_context();
    let _ = validate_document_by_schema_context(&doc, &ctx);
    let validation_time = validation_start.elapsed();
    memory_tracker.update();

    let node_count = doc.node_count();
    let peak_memory = memory_tracker.peak_usage();
    let schema_cache_size = schema_manager.total_cache_size();
    let schema_cache_count = schema_manager.schema_count();

    DomBenchResult {
        file_name,
        file_size,
        parse_time,
        validation_time,
        node_count,
        peak_memory,
        schema_cache_size,
        schema_cache_count,
    }
}

// =============================================================================
// Streaming Benchmark with Validation
// =============================================================================

fn bench_stream_with_validation(
    input: &str,
    content: &[u8],
    schema_manager: &SchemaManager,
) -> StreamBenchResult {
    let file_name = get_display_name(input).to_string();
    let file_size = content.len();

    let memory_tracker = Arc::new(MemoryTracker::new());

    // Streaming parse only
    let parse_start = Instant::now();
    let reader = BufReader::new(std::io::Cursor::new(content));
    let mut parser = StreamingParser::new(reader);
    let handler = CountingHandler::new(Arc::clone(&memory_tracker));
    parser.add_handler(Box::new(handler));
    let _ = parser.parse();
    let parse_time = parse_start.elapsed();
    memory_tracker.update();

    // Streaming with schema validation
    let validation_tracker = Arc::new(MemoryTracker::new());
    let validation_start = Instant::now();
    let reader = BufReader::new(std::io::Cursor::new(content));
    let mut parser = StreamingParser::new(reader);

    let schema = schema_manager.get_compiled_schema();
    let validator = StreamingSchemaValidator::new(schema);
    parser.add_handler(Box::new(validator));

    let handler = CountingHandler::new(Arc::clone(&validation_tracker));
    parser.add_handler(Box::new(handler));

    let _ = parser.parse();
    let validation_time = validation_start.elapsed();
    validation_tracker.update();

    let peak_memory = memory_tracker
        .peak_usage()
        .max(validation_tracker.peak_usage());
    let schema_cache_size = schema_manager.total_cache_size();
    let schema_cache_count = schema_manager.schema_count();

    StreamBenchResult {
        file_name,
        file_size,
        parse_time,
        validation_time,
        peak_memory,
        schema_cache_size,
        schema_cache_count,
    }
}

fn print_separator() {
    println!("{}", "=".repeat(60));
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let config = Config::from_args();

    println!();
    print_separator();
    println!("  XML Benchmark (DOM vs Streaming with Schema Validation)");
    println!("  Cache dir: {:?}", config.cache_dir);
    println!("  Inputs: {}", config.urls.len());
    print_separator();
    println!();

    // Shared schema manager
    let mut schema_manager = SchemaManager::new();

    // Load all files
    println!("--- Loading XML Files ---\n");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    for input in &config.urls {
        match load_file(input, &config.cache_dir) {
            Ok(content) => {
                println!(
                    "  OK: {} - {:.2} MB",
                    get_display_name(input),
                    content.len() as f64 / 1_000_000.0
                );
                files.push((input.clone(), content));
            }
            Err(e) => {
                println!("  SKIP: {} ({})", get_display_name(input), e);
            }
        }
    }

    let mut dom_results: Option<(usize, Duration, Duration, usize)> = None;
    let mut stream_results: Option<(usize, Duration, Duration, usize)> = None;

    // DOM Benchmarks
    if !config.streaming_only {
        println!();
        print_separator();
        println!("  DOM Parsing Benchmark");
        print_separator();
        println!();

        let mut dom_total_size = 0usize;
        let mut dom_total_time = Duration::ZERO;
        let mut dom_total_validation_time = Duration::ZERO;
        let mut dom_peak_memory = 0usize;

        for (url, content) in &files {
            let result = bench_dom(url, content, &mut schema_manager);

            println!("File: {}", result.file_name);
            println!(
                "  Size:               {:.2} MB",
                result.file_size as f64 / 1_000_000.0
            );
            println!(
                "  Parse only:         {:?} ({:.2} MB/s)",
                result.parse_time,
                result.throughput_mbps()
            );
            println!(
                "  Parse + Validate:   {:?} ({:.2} MB/s)",
                result.parse_time + result.validation_time,
                result.file_size as f64
                    / (result.parse_time + result.validation_time).as_secs_f64()
                    / 1_000_000.0
            );
            println!("  Validation only:    {:?}", result.validation_time);
            println!("  Node count:         {}", result.node_count);
            println!(
                "  Peak memory:        {:.2} MB",
                result.peak_memory as f64 / 1_000_000.0
            );
            println!(
                "  Schema cache:       {:.2} KB ({} schemas)",
                result.schema_cache_size as f64 / 1_000.0,
                result.schema_cache_count
            );
            println!();

            dom_total_size += result.file_size;
            dom_total_time += result.parse_time;
            dom_total_validation_time += result.validation_time;
            dom_peak_memory = dom_peak_memory.max(result.peak_memory);
        }

        println!("--- DOM Summary ---");
        println!(
            "  Total size:           {:.2} MB",
            dom_total_size as f64 / 1_000_000.0
        );
        println!("  Total parse time:     {:?}", dom_total_time);
        println!("  Total validate time:  {:?}", dom_total_validation_time);
        println!(
            "  Parse throughput:     {:.2} MB/s",
            dom_total_size as f64 / dom_total_time.as_secs_f64() / 1_000_000.0
        );
        println!(
            "  Validate throughput:  {:.2} MB/s",
            dom_total_size as f64
                / (dom_total_time + dom_total_validation_time).as_secs_f64()
                / 1_000_000.0
        );
        println!(
            "  Peak memory:          {:.2} MB",
            dom_peak_memory as f64 / 1_000_000.0
        );
        println!(
            "  Schema cache:         {:.2} KB ({} schemas)",
            schema_manager.total_cache_size() as f64 / 1_000.0,
            schema_manager.schema_count()
        );

        dom_results = Some((
            dom_total_size,
            dom_total_time,
            dom_total_validation_time,
            dom_peak_memory,
        ));
    }

    // Streaming Benchmarks
    if !config.dom_only {
        println!();
        print_separator();
        println!("  Streaming Benchmark (with Schema Validation)");
        print_separator();
        println!();

        let mut stream_total_size = 0usize;
        let mut stream_total_parse_time = Duration::ZERO;
        let mut stream_total_validation_time = Duration::ZERO;
        let mut stream_peak_memory = 0usize;

        for (url, content) in &files {
            let result = bench_stream_with_validation(url, content, &schema_manager);

            println!("File: {}", result.file_name);
            println!(
                "  Size:               {:.2} MB",
                result.file_size as f64 / 1_000_000.0
            );
            println!(
                "  Parse only:         {:?} ({:.2} MB/s)",
                result.parse_time,
                result.throughput_mbps()
            );
            println!(
                "  Parse + Validate:   {:?} ({:.2} MB/s)",
                result.validation_time,
                result.validation_throughput_mbps()
            );
            println!(
                "  Validation overhead: {:.1}%",
                (result.validation_time.as_secs_f64() / result.parse_time.as_secs_f64() - 1.0)
                    * 100.0
            );
            println!(
                "  Peak memory:        {:.2} MB",
                result.peak_memory as f64 / 1_000_000.0
            );
            println!(
                "  Schema cache:       {:.2} KB ({} schemas)",
                result.schema_cache_size as f64 / 1_000.0,
                result.schema_cache_count
            );
            println!();

            stream_total_size += result.file_size;
            stream_total_parse_time += result.parse_time;
            stream_total_validation_time += result.validation_time;
            stream_peak_memory = stream_peak_memory.max(result.peak_memory);
        }

        println!("--- Streaming Summary ---");
        println!(
            "  Total size:           {:.2} MB",
            stream_total_size as f64 / 1_000_000.0
        );
        println!("  Total parse time:     {:?}", stream_total_parse_time);
        println!("  Total validate time:  {:?}", stream_total_validation_time);
        println!(
            "  Parse throughput:     {:.2} MB/s",
            stream_total_size as f64 / stream_total_parse_time.as_secs_f64() / 1_000_000.0
        );
        println!(
            "  Validate throughput:  {:.2} MB/s",
            stream_total_size as f64 / stream_total_validation_time.as_secs_f64() / 1_000_000.0
        );
        println!(
            "  Validation overhead:  {:.1}%",
            (stream_total_validation_time.as_secs_f64() / stream_total_parse_time.as_secs_f64()
                - 1.0)
                * 100.0
        );
        println!(
            "  Peak memory:          {:.2} MB",
            stream_peak_memory as f64 / 1_000_000.0
        );

        stream_results = Some((
            stream_total_size,
            stream_total_parse_time,
            stream_total_validation_time,
            stream_peak_memory,
        ));
    }

    // Final comparison (if both modes were run)
    if let (
        Some((dom_total_size, dom_total_time, dom_total_validation_time, dom_peak_memory)),
        Some((
            _stream_total_size,
            stream_total_parse_time,
            stream_total_validation_time,
            stream_peak_memory,
        )),
    ) = (dom_results, stream_results)
    {
        println!();
        print_separator();
        println!("  Final Comparison");
        print_separator();
        println!();
        println!("                        DOM             Streaming       Ratio");
        println!(
            "  Parse only:           {:>12?}    {:>12?}    {:.2}x",
            dom_total_time,
            stream_total_parse_time,
            dom_total_time.as_secs_f64() / stream_total_parse_time.as_secs_f64()
        );
        println!(
            "  With validation:      {:>12?}    {:>12?}    {:.2}x",
            dom_total_time + dom_total_validation_time,
            stream_total_validation_time,
            (dom_total_time + dom_total_validation_time).as_secs_f64()
                / stream_total_validation_time.as_secs_f64()
        );
        println!(
            "  Parse throughput:     {:>8.2} MB/s    {:>8.2} MB/s",
            dom_total_size as f64 / dom_total_time.as_secs_f64() / 1_000_000.0,
            dom_total_size as f64 / stream_total_parse_time.as_secs_f64() / 1_000_000.0
        );
        println!(
            "  Validate throughput:  {:>8.2} MB/s    {:>8.2} MB/s",
            dom_total_size as f64
                / (dom_total_time + dom_total_validation_time).as_secs_f64()
                / 1_000_000.0,
            dom_total_size as f64 / stream_total_validation_time.as_secs_f64() / 1_000_000.0
        );
        println!(
            "  Peak memory:          {:>8.2} MB      {:>8.2} MB      {:.1}x",
            dom_peak_memory as f64 / 1_000_000.0,
            stream_peak_memory as f64 / 1_000_000.0,
            dom_peak_memory as f64 / stream_peak_memory.max(1) as f64
        );
    }

    // Cache stats
    let file_cache_size: u64 = fs::read_dir(&config.cache_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);

    println!();
    println!(
        "  File cache (disk):    {:.2} MB ({:?})",
        file_cache_size as f64 / 1_000_000.0,
        config.cache_dir
    );
    println!(
        "  Schema cache (temp):  {:.2} KB ({} schemas)",
        schema_manager.total_cache_size() as f64 / 1_000.0,
        schema_manager.schema_count()
    );
    println!("  Schema cache path:    {:?}", schema_manager.cache_path());
}
