//! Load testing CLI for fastxml.
//!
//! This tool measures parsing performance with either generated or real XML files.
//!
//! # Usage
//!
//! ## Synthetic data (generated XML)
//! ```bash
//! cargo run --release --example load_test_cli -- --pattern many-elements --size 10000
//! cargo run --release --example load_test_cli -- --pattern citygml --size 1000
//! ```
//!
//! ## Real files (URLs or local paths)
//! ```bash
//! # Local files
//! cargo run --release --example load_test_cli -- ./file1.xml ./file2.xml
//!
//! # URLs (requires sync feature)
//! cargo run --release --example load_test_cli --features sync -- \
//!     https://example.com/file.xml
//!
//! # From stdin
//! cat urls.txt | cargo run --release --example load_test_cli --features sync
//! ```
//!
//! ## Comparison with libxml
//! ```bash
//! # With libxml comparison
//! cargo run --release --example load_test_cli --features compare-libxml -- ./file.xml
//! ```
//!
//! ## Options
//! ```bash
//! --pattern <PATTERN>   Test pattern: many-elements, deep-nesting, large-content, citygml
//! --size <SIZE>         Size parameter for pattern
//! --mode <MODE>         Processing mode: dom, streaming, both (default)
//! --iterations <N>      Number of iterations (default: 3)
//! --validate            Enable schema validation benchmark
//! --cache-dir <DIR>     Cache directory for downloaded URLs (default: examples/cache)
//! ```

use std::fs;
#[cfg(feature = "ureq")]
use std::io::Write;
use std::io::{BufRead, BufReader, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::generator::{GeneratorConfig, XmlStreamGenerator};
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::validator::StreamingSchemaValidator;
use fastxml::schema::xsd::create_builtin_schema;
#[cfg(feature = "ureq")]
use fastxml::schema::{DefaultFetcher, export::export_schemas_from_xml};
use fastxml::{evaluate, parse};

// =============================================================================
// Configuration
// =============================================================================

struct Config {
    mode: Mode,
    processing_mode: String,
    iterations: usize,
    validate: bool,
    cache_dir: PathBuf,
}

enum Mode {
    Pattern { pattern: String, size: usize },
    Files { inputs: Vec<String> },
}

impl Config {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();

        let mut pattern: Option<String> = None;
        let mut size = 10_000usize;
        let mut processing_mode = "both".to_string();
        let mut iterations = 3usize;
        let mut validate = false;
        let mut cache_dir = PathBuf::from("examples/cache");
        let mut inputs: Vec<String> = Vec::new();
        let mut show_help = false;

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => show_help = true,
                "--pattern" => {
                    i += 1;
                    if i < args.len() {
                        pattern = Some(args[i].clone());
                    }
                }
                "--size" => {
                    i += 1;
                    if i < args.len() {
                        size = args[i].parse().unwrap_or(10_000);
                    }
                }
                "--mode" => {
                    i += 1;
                    if i < args.len() {
                        processing_mode = args[i].clone();
                    }
                }
                "--iterations" => {
                    i += 1;
                    if i < args.len() {
                        iterations = args[i].parse().unwrap_or(3);
                    }
                }
                "--validate" => validate = true,
                "--cache-dir" => {
                    i += 1;
                    if i < args.len() {
                        cache_dir = PathBuf::from(&args[i]);
                    }
                }
                arg if !arg.starts_with('-') => {
                    inputs.push(arg.to_string());
                }
                _ => {
                    eprintln!("Unknown option: {}", args[i]);
                    std::process::exit(1);
                }
            }
            i += 1;
        }

        if show_help {
            print_help(&args[0]);
            std::process::exit(0);
        }

        // Read from stdin if no inputs and not a terminal
        if inputs.is_empty() && pattern.is_none() && !std::io::stdin().is_terminal() {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().map_while(|l| l.ok()) {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    inputs.push(line.to_string());
                }
            }
        }

        let mode = if let Some(p) = pattern {
            Mode::Pattern { pattern: p, size }
        } else if !inputs.is_empty() {
            Mode::Files { inputs }
        } else {
            // Default to pattern mode
            Mode::Pattern {
                pattern: "many-elements".to_string(),
                size,
            }
        };

        Self {
            mode,
            processing_mode,
            iterations,
            validate,
            cache_dir,
        }
    }
}

fn print_help(program: &str) {
    eprintln!("fastxml Load Test CLI");
    eprintln!();
    eprintln!("Usage: {} [OPTIONS] [FILES...]", program);
    eprintln!();
    eprintln!("Modes:");
    eprintln!(
        "  Synthetic data:  {} --pattern <PATTERN> --size <SIZE>",
        program
    );
    eprintln!("  Real files:      {} file1.xml file2.xml", program);
    eprintln!("  From stdin:      cat urls.txt | {}", program);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --pattern <PATTERN>   Test pattern (synthetic mode):");
    eprintln!("                        - many-elements: wide tree with many sibling elements");
    eprintln!("                        - deep-nesting: deeply nested elements");
    eprintln!("                        - large-content: elements with large text content");
    eprintln!("                        - citygml: CityGML-style document with namespaces");
    eprintln!("  --size <SIZE>         Size parameter for pattern (default: 10000)");
    eprintln!("  --mode <MODE>         Processing mode: dom, streaming, both (default: both)");
    eprintln!("  --iterations <N>      Number of iterations (default: 3)");
    eprintln!("  --validate            Enable schema validation benchmark");
    eprintln!("  --cache-dir <DIR>     Cache directory for URLs (default: examples/cache)");
    eprintln!("  -h, --help            Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  # Synthetic: 100k elements");
    eprintln!("  {} --pattern many-elements --size 100000", program);
    eprintln!();
    eprintln!("  # Synthetic: CityGML with 1000 buildings");
    eprintln!("  {} --pattern citygml --size 1000 --validate", program);
    eprintln!();
    eprintln!("  # Real files");
    eprintln!("  {} ./large.xml https://example.com/data.xml", program);
}

// =============================================================================
// Handlers
// =============================================================================

struct StatsHandler {
    element_count: usize,
    max_depth: usize,
    current_depth: usize,
    text_bytes: usize,
    attr_count: usize,
}

impl StatsHandler {
    fn new() -> Self {
        Self {
            element_count: 0,
            max_depth: 0,
            current_depth: 0,
            text_bytes: 0,
            attr_count: 0,
        }
    }
}

impl XmlEventHandler for StatsHandler {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement { attributes, .. } => {
                self.element_count += 1;
                self.attr_count += attributes.len();
                self.current_depth += 1;
                self.max_depth = self.max_depth.max(self.current_depth);
            }
            XmlEvent::EndElement { .. } => {
                self.current_depth = self.current_depth.saturating_sub(1);
            }
            XmlEvent::Text(s) | XmlEvent::CData(s) => {
                self.text_bytes += s.len();
            }
            _ => {}
        }
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

struct CountingReader<R> {
    inner: R,
    bytes_read: usize,
}

impl<R> CountingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            bytes_read: 0,
        }
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes_read += n;
        Ok(n)
    }
}

impl<R: BufRead> BufRead for CountingReader<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.bytes_read += amt;
        self.inner.consume(amt);
    }
}

// =============================================================================
// Utilities
// =============================================================================

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_duration(d: Duration) -> String {
    if d.as_secs() > 0 {
        format!("{:.2}s", d.as_secs_f64())
    } else if d.as_millis() > 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{}µs", d.as_micros())
    }
}

fn get_memory_usage() -> Option<usize> {
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let pid = std::process::id();
        let output = Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let rss = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<usize>()
            .ok()?;
        Some(rss * 1024)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn print_separator() {
    println!("{}", "=".repeat(60));
}

// =============================================================================
// libxml Comparison
// =============================================================================

#[cfg(feature = "compare-libxml")]
mod libxml_bench {
    use std::time::{Duration, Instant};

    /// Parse XML with libxml and return timing info
    pub fn parse_with_libxml(
        content: &[u8],
        iterations: usize,
        get_memory: fn() -> Option<usize>,
    ) -> Option<LibxmlResult> {
        let xml_str = std::str::from_utf8(content).ok()?;
        let parser = libxml::parser::Parser::default();

        let mut total_time = Duration::ZERO;
        let mut node_count = 0usize;
        let mut memory_delta = None;

        for i in 0..iterations {
            // Measure memory on first iteration
            let mem_before = if i == 0 { get_memory() } else { None };

            let start = Instant::now();
            let doc = parser.parse_string(xml_str).ok()?;
            total_time += start.elapsed();

            if i == 0 {
                let mem_after = get_memory();
                if let (Some(before), Some(after)) = (mem_before, mem_after) {
                    memory_delta = Some(after.saturating_sub(before));
                }
                // Count nodes on first iteration
                node_count = count_nodes(&doc);
            }
        }

        Some(LibxmlResult {
            avg_time: total_time / iterations as u32,
            node_count,
            size: content.len(),
            memory_delta,
        })
    }

    /// Parse and validate XML with libxml using exported schema file
    pub fn validate_with_libxml(
        content: &[u8],
        schema_path: &std::path::Path,
        iterations: usize,
        get_memory: fn() -> Option<usize>,
    ) -> Option<LibxmlValidationResult> {
        use libxml::schemas::{SchemaParserContext, SchemaValidationContext};

        let xml_str = std::str::from_utf8(content).ok()?;
        let schema_path_str = schema_path.to_str()?;
        let parser = libxml::parser::Parser::default();

        // Parse schema once to verify it works
        let mut schema_parser = SchemaParserContext::from_file(schema_path_str);
        let schema_ctx = SchemaValidationContext::from_parser(&mut schema_parser);
        if schema_ctx.is_err() {
            eprintln!("    libxml: Failed to parse schema from {:?}", schema_path);
            return None;
        }

        let mut total_time = Duration::ZERO;
        let mut memory_delta = None;
        let mut validation_errors = 0usize;

        for i in 0..iterations {
            // Need fresh schema context for each validation
            let mut schema_parser = SchemaParserContext::from_file(schema_path_str);
            let schema_ctx = SchemaValidationContext::from_parser(&mut schema_parser);
            let mut schema_ctx = match schema_ctx {
                Ok(ctx) => ctx,
                Err(_) => return None,
            };

            let mem_before = if i == 0 { get_memory() } else { None };

            let start = Instant::now();
            let doc = parser.parse_string(xml_str).ok()?;
            if let Err(errors) = schema_ctx.validate_document(&doc)
                && i == 0
            {
                validation_errors = errors.len();
            }
            total_time += start.elapsed();

            if i == 0 {
                let mem_after = get_memory();
                if let (Some(before), Some(after)) = (mem_before, mem_after) {
                    memory_delta = Some(after.saturating_sub(before));
                }
            }
        }

        Some(LibxmlValidationResult {
            avg_time: total_time / iterations as u32,
            size: content.len(),
            memory_delta,
            validation_errors,
        })
    }

    fn count_nodes(doc: &libxml::tree::Document) -> usize {
        fn count_recursive(node: &libxml::tree::Node) -> usize {
            let mut count = 1;
            for child in node.get_child_nodes() {
                count += count_recursive(&child);
            }
            count
        }

        doc.get_root_element()
            .map(|root| count_recursive(&root))
            .unwrap_or(0)
    }

    pub struct LibxmlResult {
        pub avg_time: Duration,
        pub node_count: usize,
        pub size: usize,
        pub memory_delta: Option<usize>,
    }

    impl LibxmlResult {
        pub fn throughput_mb_s(&self) -> f64 {
            self.size as f64 / self.avg_time.as_secs_f64() / (1024.0 * 1024.0)
        }
    }

    pub struct LibxmlValidationResult {
        pub avg_time: Duration,
        pub size: usize,
        pub memory_delta: Option<usize>,
        pub validation_errors: usize,
    }

    impl LibxmlValidationResult {
        pub fn throughput_mb_s(&self) -> f64 {
            self.size as f64 / self.avg_time.as_secs_f64() / (1024.0 * 1024.0)
        }
    }
}

// =============================================================================
// File Loading
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

#[cfg(feature = "ureq")]
fn load_file(
    input: &str,
    cache_dir: &Path,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    if is_url(input) {
        let file_name = input.split('/').next_back().unwrap_or("unknown.xml");
        let cache_path = cache_dir.join(file_name);

        if cache_path.exists() {
            return Ok(fs::read(&cache_path)?);
        }

        println!("  Downloading: {}", input);
        let response = ureq::get(input)
            .timeout(std::time::Duration::from_secs(60))
            .call()?;

        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes)?;

        fs::create_dir_all(cache_dir)?;
        let mut file = fs::File::create(&cache_path)?;
        file.write_all(&bytes)?;

        Ok(bytes)
    } else {
        Ok(fs::read(input)?)
    }
}

#[cfg(not(feature = "ureq"))]
fn load_file(
    input: &str,
    _cache_dir: &Path,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    if is_url(input) {
        Err("URL loading requires 'sync' feature. Use: cargo run --features sync --example load_test_cli".into())
    } else {
        Ok(fs::read(input)?)
    }
}

// =============================================================================
// Pattern Mode Benchmark
// =============================================================================

fn run_pattern_test(
    config: GeneratorConfig,
    processing_mode: &str,
    iterations: usize,
    validate: bool,
) {
    println!();
    print_separator();
    println!("Configuration (Synthetic):");
    println!("  Elements:     {:>10}", config.element_count);
    println!("  Max Depth:    {:>10}", config.max_depth);
    println!("  Content Size: {:>10}", format_bytes(config.content_size));
    println!("  Attributes:   {:>10}/element", config.attribute_count);
    println!("  Namespaces:   {:>10}", config.with_namespaces);
    println!(
        "  Est. Size:    {:>10}",
        format_bytes(config.estimated_size())
    );
    print_separator();

    // Generate XML once for DOM tests
    let xml_bytes = if processing_mode == "streaming" {
        Vec::new()
    } else {
        println!("\nGenerating XML...");
        let start = Instant::now();
        let mut xml_gen = XmlStreamGenerator::new(config.clone());
        let mut bytes = Vec::new();
        xml_gen.read_to_end(&mut bytes).unwrap();
        println!(
            "  Generated {} in {}",
            format_bytes(bytes.len()),
            format_duration(start.elapsed())
        );
        bytes
    };

    let schema = if validate {
        Some(Arc::new(create_builtin_schema()))
    } else {
        None
    };

    // DOM parsing test
    if processing_mode == "dom" || processing_mode == "both" {
        run_dom_benchmark(&xml_bytes, iterations, None);

        // XPath test
        println!("\n--- XPath Evaluation ---");
        let doc = parse(&xml_bytes).unwrap();

        let start = Instant::now();
        let result = evaluate(&doc, "//*").unwrap();
        let count = result.into_nodes().len();
        println!(
            "  //*: {} elements in {}",
            count,
            format_duration(start.elapsed())
        );

        if config.with_namespaces {
            let start = Instant::now();
            let result = evaluate(&doc, "//bldg:Building").unwrap();
            let count = result.into_nodes().len();
            println!(
                "  //bldg:Building: {} elements in {}",
                count,
                format_duration(start.elapsed())
            );
        }
    }

    // Streaming test
    if processing_mode == "streaming" || processing_mode == "both" {
        println!("\n--- Streaming Parse ---");
        let mem_before = get_memory_usage();

        let mut total_time = Duration::ZERO;
        let mut total_bytes = 0usize;

        for i in 0..iterations {
            let xml_gen = XmlStreamGenerator::new(config.clone());
            let reader = BufReader::with_capacity(64 * 1024, xml_gen);

            let start = Instant::now();
            let mut counting_reader = CountingReader::new(reader);
            let mut parser = StreamingParser::new(&mut counting_reader);
            let handler = StatsHandler::new();
            parser.add_handler(Box::new(handler));
            if let Some(ref s) = schema {
                let validator = StreamingSchemaValidator::new(Arc::clone(s));
                parser.add_handler(Box::new(validator));
            }
            let _ = parser.parse();

            total_time += start.elapsed();
            total_bytes = counting_reader.bytes_read;

            if i == 0 {
                println!("  Processed: {}", format_bytes(total_bytes));
            }
        }

        let mem_after = get_memory_usage();
        let avg_time = total_time / iterations as u32;
        let throughput = total_bytes as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

        println!("  Avg Time:    {}", format_duration(avg_time));
        println!("  Throughput:  {:.2} MB/s", throughput);

        if let (Some(before), Some(after)) = (mem_before, mem_after) {
            println!(
                "  Memory:      {} -> {} (Δ {})",
                format_bytes(before),
                format_bytes(after),
                format_bytes(after.saturating_sub(before))
            );
        }
    }
}

// =============================================================================
// File Mode Benchmark
// =============================================================================

fn run_file_benchmark(
    name: &str,
    file_path: Option<&str>,
    content: &[u8],
    processing_mode: &str,
    iterations: usize,
    validate: bool,
) {
    println!("\n--- {} ({}) ---", name, format_bytes(content.len()));

    // Get schema from xsi:schemaLocation if validation is enabled
    let schema_info: Option<SchemaInfo> = if validate {
        get_schema_from_content(content, file_path)
    } else {
        None
    };

    // DOM
    if processing_mode == "dom" || processing_mode == "both" {
        run_dom_benchmark(content, iterations, schema_info.as_ref());
    }

    // Streaming
    if processing_mode == "streaming" || processing_mode == "both" {
        run_streaming_benchmark(
            content,
            iterations,
            schema_info.as_ref().map(|s| &s.compiled),
        );
    }
}

/// Schema info with compiled schema and exported schema directory
struct SchemaInfo {
    compiled: Arc<CompiledSchema>,
    /// Directory containing exported schemas (for libxml comparison)
    #[allow(dead_code)]
    export_dir: Option<PathBuf>,
    /// Entry schema filename in export_dir
    #[allow(dead_code)]
    entry_filename: Option<String>,
}

/// Extracts schema from XML content using xsi:schemaLocation.
/// Exports all schemas to a temp directory for fair comparison with libxml.
#[cfg(feature = "ureq")]
fn get_schema_from_content(content: &[u8], xml_file_path: Option<&str>) -> Option<SchemaInfo> {
    use fastxml::schema::xsd::parse_xsd_multiple;

    // Create temp directory for exported schemas
    let export_dir = std::env::temp_dir().join(format!("fastxml_schemas_{}", std::process::id()));
    let _ = fs::remove_dir_all(&export_dir);

    // Create fetcher with base directory from XML file
    let fetcher = if let Some(path) = xml_file_path {
        let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
        DefaultFetcher::with_base_dir(base_dir)
    } else {
        DefaultFetcher::new()
    };

    // Export schemas
    print!("  Exporting schemas... ");
    match export_schemas_from_xml(content, &export_dir, &fetcher) {
        Ok(result) => {
            if result.schema_count == 0 {
                println!("no schemas found, using built-in schema");
                return Some(SchemaInfo {
                    compiled: Arc::new(create_builtin_schema()),
                    export_dir: None,
                    entry_filename: None,
                });
            }
            println!(
                "exported {} schemas to {:?}",
                result.schema_count, export_dir
            );

            if let Some(ref entry) = result.entry_filename {
                println!("  Entry schema: {}", entry);
            }

            // Parse exported schemas with fastxml
            print!("  Compiling schemas for fastxml... ");
            let mut xsd_contents: Vec<(String, Vec<u8>)> = Vec::new();
            for (uri, filename) in &result.uri_to_filename {
                let path = export_dir.join(filename);
                if let Ok(content) = fs::read(&path) {
                    xsd_contents.push((uri.clone(), content));
                }
            }
            let xsd_refs: Vec<(&str, &[u8])> = xsd_contents
                .iter()
                .map(|(uri, content)| (uri.as_str(), content.as_slice()))
                .collect();

            match parse_xsd_multiple(&xsd_refs) {
                Ok(schema) => {
                    println!("OK ({} types)", schema.types.len());
                    Some(SchemaInfo {
                        compiled: Arc::new(schema),
                        export_dir: Some(export_dir),
                        entry_filename: result.entry_filename,
                    })
                }
                Err(e) => {
                    eprintln!("FAILED: {}", e);
                    println!("  Falling back to built-in schema");
                    Some(SchemaInfo {
                        compiled: Arc::new(create_builtin_schema()),
                        export_dir: Some(export_dir),
                        entry_filename: result.entry_filename,
                    })
                }
            }
        }
        Err(e) => {
            eprintln!("FAILED: {}", e);
            println!("  Falling back to built-in schema");
            Some(SchemaInfo {
                compiled: Arc::new(create_builtin_schema()),
                export_dir: None,
                entry_filename: None,
            })
        }
    }
}

#[cfg(not(feature = "ureq"))]
fn get_schema_from_content(_content: &[u8], _xml_file_path: Option<&str>) -> Option<SchemaInfo> {
    println!("  Note: Schema fetching requires 'ureq' feature, using built-in schema");
    Some(SchemaInfo {
        compiled: Arc::new(create_builtin_schema()),
        export_dir: None,
        entry_filename: None,
    })
}

fn run_dom_benchmark(content: &[u8], iterations: usize, schema_info: Option<&SchemaInfo>) {
    println!("\n  [DOM]");

    let mut total_parse_time = Duration::ZERO;
    let mut fastxml_mem_delta: Option<usize> = None;

    for i in 0..iterations {
        // Measure memory on first iteration (keep doc alive for measurement)
        if i == 0 {
            let mem_before = get_memory_usage();
            let start = Instant::now();
            let doc = parse(content).unwrap();
            total_parse_time += start.elapsed();
            let mem_after = get_memory_usage();

            println!("    fastxml nodes: {}", doc.node_count());

            if let (Some(before), Some(after)) = (mem_before, mem_after) {
                fastxml_mem_delta = Some(after.saturating_sub(before));
            }
            // doc drops here after memory measurement
        } else {
            let start = Instant::now();
            let _doc = parse(content).unwrap();
            total_parse_time += start.elapsed();
        }
    }

    let avg_parse = total_parse_time / iterations as u32;
    let throughput = content.len() as f64 / avg_parse.as_secs_f64() / (1024.0 * 1024.0);

    println!(
        "    fastxml:    {} ({:.2} MB/s)",
        format_duration(avg_parse),
        throughput
    );

    if let Some(mem) = fastxml_mem_delta {
        println!("    fastxml mem: Δ {}", format_bytes(mem));
    }

    // libxml comparison
    #[cfg(feature = "compare-libxml")]
    {
        if let Some(libxml_result) =
            libxml_bench::parse_with_libxml(content, iterations, get_memory_usage)
        {
            println!(
                "    libxml:     {} ({:.2} MB/s)",
                format_duration(libxml_result.avg_time),
                libxml_result.throughput_mb_s()
            );
            println!("    libxml nodes: {}", libxml_result.node_count);
            if let Some(mem) = libxml_result.memory_delta {
                println!("    libxml mem: Δ {}", format_bytes(mem));
            }

            // Comparison
            println!();
            println!("    [Comparison]");
            let speedup = libxml_result.avg_time.as_secs_f64() / avg_parse.as_secs_f64();
            if speedup >= 1.0 {
                println!("    Speed:  fastxml is {:.2}x faster", speedup);
            } else {
                println!("    Speed:  libxml is {:.2}x faster", 1.0 / speedup);
            }

            // Memory comparison
            if let (Some(fastxml_mem), Some(libxml_mem)) =
                (fastxml_mem_delta, libxml_result.memory_delta)
                && fastxml_mem > 0
                && libxml_mem > 0
            {
                let mem_ratio = libxml_mem as f64 / fastxml_mem as f64;
                if mem_ratio >= 1.0 {
                    println!("    Memory: fastxml uses {:.2}x less", mem_ratio);
                } else {
                    println!("    Memory: libxml uses {:.2}x less", 1.0 / mem_ratio);
                }
            }
        }

        // libxml validation comparison (if schema is available)
        if let Some(info) = schema_info
            && let Some(ref export_dir) = info.export_dir
            && let Some(ref entry_filename) = info.entry_filename
        {
            let schema_path = export_dir.join(entry_filename);
            println!();
            println!("    [Validation Comparison]");
            println!("    Schema: {:?}", schema_path);
            if let Some(libxml_val_result) = libxml_bench::validate_with_libxml(
                content,
                &schema_path,
                iterations,
                get_memory_usage,
            ) {
                println!(
                    "    libxml + validate: {} ({:.2} MB/s)",
                    format_duration(libxml_val_result.avg_time),
                    libxml_val_result.throughput_mb_s()
                );
                if libxml_val_result.validation_errors > 0 {
                    println!(
                        "    libxml validation errors: {}",
                        libxml_val_result.validation_errors
                    );
                }
                if let Some(mem) = libxml_val_result.memory_delta {
                    println!("    libxml val mem: Δ {}", format_bytes(mem));
                }
            }
        }
    }

    // Suppress unused variable warning when compare-libxml is not enabled
    let _ = schema_info;
}

fn run_streaming_benchmark(
    content: &[u8],
    iterations: usize,
    schema: Option<&Arc<CompiledSchema>>,
) {
    println!("\n  [Streaming]");
    let mem_before = get_memory_usage();

    // Parse only
    let mut total_parse_time = Duration::ZERO;
    for _ in 0..iterations {
        let reader = BufReader::new(std::io::Cursor::new(content));
        let start = Instant::now();
        let mut parser = StreamingParser::new(reader);
        let handler = StatsHandler::new();
        parser.add_handler(Box::new(handler));
        let _ = parser.parse();
        total_parse_time += start.elapsed();
    }

    let avg_parse = total_parse_time / iterations as u32;
    let parse_throughput = content.len() as f64 / avg_parse.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "    Parse:      {} ({:.2} MB/s)",
        format_duration(avg_parse),
        parse_throughput
    );

    // With validation
    if let Some(s) = schema {
        let mut total_validate_time = Duration::ZERO;
        let mut validation_errors = Vec::new();
        for i in 0..iterations {
            let reader = BufReader::new(std::io::Cursor::new(content));
            let start = Instant::now();
            let mut parser = StreamingParser::new(reader);
            let handler = StatsHandler::new();
            parser.add_handler(Box::new(handler));
            let validator = StreamingSchemaValidator::new(Arc::clone(s));
            parser.add_handler(Box::new(validator));
            let result = parser.parse();
            total_validate_time += start.elapsed();

            // Collect errors from first iteration
            if i == 0 && result.is_ok() {
                let mut handlers = parser.into_handlers();
                // The validator is the second handler (index 1)
                if handlers.len() > 1
                    && let Some(validator) = handlers
                        .pop()
                        .map(|h| h.as_any())
                        .and_then(|h| h.downcast::<StreamingSchemaValidator>().ok())
                {
                    validation_errors = validator.into_errors();
                }
            }
        }

        let avg_validate = total_validate_time / iterations as u32;
        let validate_throughput =
            content.len() as f64 / avg_validate.as_secs_f64() / (1024.0 * 1024.0);
        println!(
            "    + Validate: {} ({:.2} MB/s)",
            format_duration(avg_validate),
            validate_throughput
        );

        let overhead = (avg_validate.as_secs_f64() / avg_parse.as_secs_f64() - 1.0) * 100.0;
        println!("    Overhead:   {:.1}%", overhead);

        // Print validation errors
        if !validation_errors.is_empty() {
            let error_count = validation_errors.iter().filter(|e| e.is_error()).count();
            let warning_count = validation_errors.iter().filter(|e| e.is_warning()).count();
            println!(
                "    Errors:     {} errors, {} warnings",
                error_count, warning_count
            );
            // Print first 10 errors
            for (i, err) in validation_errors.iter().take(10).enumerate() {
                println!("      {}: {}", i + 1, err.message);
            }
            if validation_errors.len() > 10 {
                println!("      ... and {} more", validation_errors.len() - 10);
            }
        }
    }

    let mem_after = get_memory_usage();
    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        println!(
            "    Memory:     Δ {}",
            format_bytes(after.saturating_sub(before))
        );
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let config = Config::from_args();

    println!();
    print_separator();
    println!("  fastxml Load Test CLI");
    print_separator();

    match config.mode {
        Mode::Pattern { pattern, size } => {
            println!("Mode: Synthetic ({})", pattern);
            println!(
                "Processing: {}, Iterations: {}, Validate: {}",
                config.processing_mode, config.iterations, config.validate
            );

            let gen_config = match pattern.as_str() {
                "many-elements" => GeneratorConfig::many_elements(size),
                "deep-nesting" => GeneratorConfig::deep_nesting(size),
                "large-content" => GeneratorConfig::large_content(size * 1024),
                "citygml" => GeneratorConfig::citygml_style(size),
                _ => {
                    eprintln!("Unknown pattern: {}", pattern);
                    std::process::exit(1);
                }
            };

            run_pattern_test(
                gen_config,
                &config.processing_mode,
                config.iterations,
                config.validate,
            );
        }

        Mode::Files { inputs } => {
            println!("Mode: Real files ({} inputs)", inputs.len());
            println!(
                "Processing: {}, Iterations: {}, Validate: {}",
                config.processing_mode, config.iterations, config.validate
            );

            // Load all files
            println!("\n--- Loading Files ---");
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            let mut total_size = 0usize;

            for input in &inputs {
                match load_file(input, &config.cache_dir) {
                    Ok(content) => {
                        println!(
                            "  OK: {} ({})",
                            get_display_name(input),
                            format_bytes(content.len())
                        );
                        total_size += content.len();
                        files.push((input.clone(), content));
                    }
                    Err(e) => {
                        println!("  SKIP: {} ({})", get_display_name(input), e);
                    }
                }
            }

            if files.is_empty() {
                eprintln!("\nNo files loaded!");
                std::process::exit(1);
            }

            println!(
                "\nTotal: {} files, {}",
                files.len(),
                format_bytes(total_size)
            );

            // Run benchmarks
            for (input, content) in &files {
                // Pass file path for local files (not URLs)
                let file_path = if input.starts_with("http://") || input.starts_with("https://") {
                    None
                } else {
                    Some(input.as_str())
                };
                run_file_benchmark(
                    get_display_name(input),
                    file_path,
                    content,
                    &config.processing_mode,
                    config.iterations,
                    config.validate,
                );
            }

            // Summary
            if files.len() > 1 {
                println!();
                print_separator();
                println!("  Summary");
                print_separator();
                println!("  Files:      {}", files.len());
                println!("  Total size: {}", format_bytes(total_size));
            }
        }
    }

    println!();
    print_separator();
    println!("Done!");
}
