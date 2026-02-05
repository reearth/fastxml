//! Benchmark CLI for fastxml.
//!
//! This tool measures parsing performance with either generated or real XML files.
//! For accurate memory measurement, mode=both runs each mode in a separate subprocess.
//!
//! # Usage
//!
//! ## Synthetic data (generated XML)
//! ```bash
//! cargo run --release --example bench -- --pattern many-elements --size 10000
//! cargo run --release --example bench -- --pattern citygml --size 1000
//! ```
//!
//! ## Real files (URLs or local paths)
//! ```bash
//! cargo run --release --example bench -- ./file1.xml ./file2.xml
//! cargo run --release --features ureq --example bench -- https://example.com/file.xml
//! ```
//!
//! ## With schema validation
//! ```bash
//! cargo run --release --example bench --features "ureq,compare-libxml" -- \
//!     ./file.xml --validate
//! ```

use std::fs;
use std::io::{BufRead, BufReader, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use serde_json::{Value as JsonValue, json};

use fastxml::error::Result;
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::generator::{GeneratorConfig, XmlStreamGenerator};
use fastxml::schema::XmlSchemaValidationContext;
use fastxml::schema::types::CompiledSchema;
use fastxml::schema::validator::OnePassSchemaValidator;
use fastxml::schema::xsd::create_builtin_schema;
#[cfg(feature = "ureq")]
use fastxml::schema::{DefaultFetcher, ResolveOptions, resolve_schema_from_xml};
use fastxml::transform::{StreamTransformer, StreamTransformerReader};
use fastxml::{evaluate, parse};

// =============================================================================
// CLI Arguments
// =============================================================================

#[derive(Parser, Debug)]
#[command(name = "fastxml-bench")]
#[command(about = "Benchmark CLI for fastxml", long_about = None)]
struct Args {
    /// Input files (local paths or URLs)
    inputs: Vec<String>,

    /// Test pattern for synthetic data
    #[arg(long, value_enum)]
    pattern: Option<Pattern>,

    /// Size parameter for pattern
    #[arg(long, default_value = "10000")]
    size: usize,

    /// Processing mode
    #[arg(long, value_enum, default_value = "both")]
    mode: ProcessingMode,

    /// Number of iterations
    #[arg(long, default_value = "3")]
    iterations: usize,

    /// Enable schema validation benchmark
    #[arg(long)]
    validate: bool,

    /// Cache directory for downloaded URLs
    #[arg(long, default_value = "examples/cache")]
    cache_dir: PathBuf,

    /// Internal: run single mode in subprocess (hidden from --help)
    #[arg(long = "internal-mode", hide = true)]
    internal_mode: Option<InternalMode>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Pattern {
    ManyElements,
    DeepNesting,
    LargeContent,
    Citygml,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
enum ProcessingMode {
    Dom,
    Streaming,
    Transform,
    TransformReader,
    Both,
}

/// Internal mode for subprocess execution
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
enum InternalMode {
    Dom,
    DomValidate,
    Streaming,
    StreamingValidate,
    Transform,
    TransformReader,
    #[cfg(feature = "compare-libxml")]
    Libxml,
    #[cfg(feature = "compare-libxml")]
    LibxmlValidate,
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

    #[cfg(target_os = "linux")]
    {
        // Read VmRSS from /proc/self/status
        std::fs::read_to_string("/proc/self/status")
            .ok()?
            .lines()
            .find(|line| line.starts_with("VmRSS:"))
            .and_then(|line| {
                line.split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse::<usize>().ok())
                    .map(|kb| kb * 1024)
            })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

fn print_separator() {
    println!("{}", "=".repeat(60));
}

/// Marker prefix for JSON results from subprocess
const BENCH_RESULT_PREFIX: &str = "BENCH_RESULT:";

/// Output JSON result for subprocess mode
fn output_json_result(result: &JsonValue) {
    println!("{}{}", BENCH_RESULT_PREFIX, result);
}

/// Parse JSON result from subprocess output
fn parse_subprocess_result(output: &str) -> Option<JsonValue> {
    for line in output.lines() {
        if let Some(json_str) = line.strip_prefix(BENCH_RESULT_PREFIX) {
            return serde_json::from_str(json_str).ok();
        }
    }
    None
}

/// Run a subprocess with the given internal mode
fn run_subprocess(
    internal_mode: InternalMode,
    inputs: &[String],
    iterations: usize,
    cache_dir: &Path,
) -> Option<JsonValue> {
    let exe = std::env::current_exe().ok()?;

    let mode_str = match internal_mode {
        InternalMode::Dom => "dom",
        InternalMode::DomValidate => "dom-validate",
        InternalMode::Streaming => "streaming",
        InternalMode::StreamingValidate => "streaming-validate",
        InternalMode::Transform => "transform",
        InternalMode::TransformReader => "transform-reader",
        #[cfg(feature = "compare-libxml")]
        InternalMode::Libxml => "libxml",
        #[cfg(feature = "compare-libxml")]
        InternalMode::LibxmlValidate => "libxml-validate",
    };

    let mut cmd = Command::new(exe);
    cmd.args(inputs)
        .arg("--iterations")
        .arg(iterations.to_string())
        .arg("--cache-dir")
        .arg(cache_dir)
        .arg("--internal-mode")
        .arg(mode_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    let output = cmd.output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_subprocess_result(&stdout)
}

// =============================================================================
// libxml Comparison
// =============================================================================

#[cfg(feature = "compare-libxml")]
mod libxml_bench {
    use super::*;
    use std::time::{Duration, Instant};

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
            let mem_before = if i == 0 { get_memory() } else { None };

            let start = Instant::now();
            let doc = parser.parse_string(xml_str).ok()?;
            total_time += start.elapsed();

            if i == 0 {
                let mem_after = get_memory();
                if let (Some(before), Some(after)) = (mem_before, mem_after) {
                    memory_delta = Some(after.saturating_sub(before));
                }
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

        let mut schema_parser = SchemaParserContext::from_file(schema_path_str);
        if SchemaValidationContext::from_parser(&mut schema_parser).is_err() {
            eprintln!("    libxml: Failed to parse schema from {:?}", schema_path);
            return None;
        }

        let mut total_time = Duration::ZERO;
        let mut memory_delta = None;
        let mut validation_errors = 0usize;

        for i in 0..iterations {
            let mut schema_parser = SchemaParserContext::from_file(schema_path_str);
            let mut schema_ctx = SchemaValidationContext::from_parser(&mut schema_parser).ok()?;

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

    /// Run libxml benchmark in subprocess mode (outputs JSON)
    pub fn run_libxml_subprocess(content: &[u8], iterations: usize) {
        if let Some(result) = parse_with_libxml(content, iterations, get_memory_usage) {
            output_json_result(&json!({
                "mode": "libxml",
                "parse_time_ms": result.avg_time.as_secs_f64() * 1000.0,
                "throughput_mb_s": result.throughput_mb_s(),
                "memory_delta_bytes": result.memory_delta,
                "node_count": result.node_count,
            }));
        }
    }

    /// Run libxml validation benchmark in subprocess mode (outputs JSON)
    pub fn run_libxml_validate_subprocess(
        content: &[u8],
        schema_path: &std::path::Path,
        iterations: usize,
    ) {
        if let Some(result) =
            validate_with_libxml(content, schema_path, iterations, get_memory_usage)
        {
            output_json_result(&json!({
                "mode": "libxml-validate",
                "parse_time_ms": result.avg_time.as_secs_f64() * 1000.0,
                "throughput_mb_s": result.throughput_mb_s(),
                "memory_delta_bytes": result.memory_delta,
                "validation_errors": result.validation_errors,
            }));
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
    use std::io::Write;

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
        Err("URL loading requires 'ureq' feature".into())
    } else {
        Ok(fs::read(input)?)
    }
}

// =============================================================================
// Schema Resolution
// =============================================================================

/// Schema info for benchmarking
struct SchemaInfo {
    compiled: Arc<CompiledSchema>,
    #[allow(dead_code)]
    export_dir: Option<PathBuf>,
    #[allow(dead_code)]
    entry_filename: Option<String>,
}

#[cfg(feature = "ureq")]
fn get_schema_from_content(content: &[u8], xml_file_path: Option<&str>) -> Option<SchemaInfo> {
    let options = if let Some(path) = xml_file_path {
        let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
        ResolveOptions::with_base_dir(base_dir)
    } else {
        ResolveOptions::default()
    };

    let fetcher = if let Some(path) = xml_file_path {
        let base_dir = Path::new(path).parent().unwrap_or(Path::new("."));
        DefaultFetcher::with_base_dir(base_dir)
    } else {
        DefaultFetcher::new()
    };

    print!("  Resolving schemas... ");
    match resolve_schema_from_xml(content, &fetcher, &options) {
        Ok(resolved) => {
            if resolved.is_builtin() {
                println!("no schemas found, using built-in schema");
            } else {
                println!(
                    "resolved {} schemas ({} types)",
                    resolved
                        .export_result
                        .as_ref()
                        .map(|r| r.schema_count)
                        .unwrap_or(0),
                    resolved.compiled.types.len()
                );
                if let Some(ref entry) = resolved.entry_filename {
                    println!("  Entry schema: {}", entry);
                }
            }
            Some(SchemaInfo {
                compiled: resolved.compiled,
                export_dir: resolved.export_dir,
                entry_filename: resolved.entry_filename,
            })
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

// =============================================================================
// Subprocess Mode Benchmarks (output JSON)
// =============================================================================

/// Run DOM benchmark in subprocess mode (outputs JSON)
fn run_dom_subprocess(content: &[u8], iterations: usize) {
    let mut total_parse_time = Duration::ZERO;
    let mut node_count = 0usize;
    let mut memory_delta: Option<usize> = None;

    for i in 0..iterations {
        if i == 0 {
            // Measure memory only on first iteration, while doc is still alive
            let mem_before = get_memory_usage();
            let start = Instant::now();
            let doc = parse(content).unwrap();
            total_parse_time += start.elapsed();
            let mem_after = get_memory_usage();

            node_count = doc.node_count();
            if let (Some(before), Some(after)) = (mem_before, mem_after) {
                memory_delta = Some(after.saturating_sub(before));
            }
        } else {
            let start = Instant::now();
            let _doc = parse(content).unwrap();
            total_parse_time += start.elapsed();
        }
    }

    let avg_parse = total_parse_time / iterations as u32;
    let throughput = content.len() as f64 / avg_parse.as_secs_f64() / (1024.0 * 1024.0);

    output_json_result(&json!({
        "mode": "dom",
        "parse_time_ms": avg_parse.as_secs_f64() * 1000.0,
        "throughput_mb_s": throughput,
        "memory_delta_bytes": memory_delta,
        "node_count": node_count,
    }));
}

/// Run DOM + validation benchmark in subprocess mode (outputs JSON)
fn run_dom_validate_subprocess(content: &[u8], iterations: usize, file_path: Option<&str>) {
    let schema_info = get_schema_from_content(content, file_path);
    let Some(info) = schema_info else {
        return;
    };

    let ctx = XmlSchemaValidationContext::from_arc(Arc::clone(&info.compiled));
    let mut total_time = Duration::ZERO;
    let mut validation_errors = 0usize;
    let mut node_count = 0usize;
    let mut memory_delta: Option<usize> = None;

    for i in 0..iterations {
        if i == 0 {
            // Measure memory only on first iteration, while doc is still alive
            let mem_before = get_memory_usage();
            let start = Instant::now();
            let doc = parse(content).unwrap();
            let errors = ctx.validate(&doc).unwrap_or_default();
            total_time += start.elapsed();
            let mem_after = get_memory_usage();

            validation_errors = errors.iter().filter(|e| e.is_error()).count();
            node_count = doc.node_count();
            if let (Some(before), Some(after)) = (mem_before, mem_after) {
                memory_delta = Some(after.saturating_sub(before));
            }
        } else {
            let start = Instant::now();
            let doc = parse(content).unwrap();
            let _errors = ctx.validate(&doc).unwrap_or_default();
            total_time += start.elapsed();
        }
    }

    let avg_time = total_time / iterations as u32;
    let throughput = content.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    output_json_result(&json!({
        "mode": "dom-validate",
        "parse_time_ms": avg_time.as_secs_f64() * 1000.0,
        "throughput_mb_s": throughput,
        "memory_delta_bytes": memory_delta,
        "node_count": node_count,
        "validation_errors": validation_errors,
    }));
}

/// Run streaming benchmark in subprocess mode (outputs JSON)
fn run_streaming_subprocess(content: &[u8], iterations: usize) {
    let mem_before = get_memory_usage();
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

    let mem_after = get_memory_usage();
    let avg_parse = total_parse_time / iterations as u32;
    let throughput = content.len() as f64 / avg_parse.as_secs_f64() / (1024.0 * 1024.0);

    let memory_delta = match (mem_before, mem_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    output_json_result(&json!({
        "mode": "streaming",
        "parse_time_ms": avg_parse.as_secs_f64() * 1000.0,
        "throughput_mb_s": throughput,
        "memory_delta_bytes": memory_delta,
    }));
}

/// Run streaming + validation benchmark in subprocess mode (outputs JSON)
fn run_streaming_validate_subprocess(content: &[u8], iterations: usize, file_path: Option<&str>) {
    let schema_info = get_schema_from_content(content, file_path);
    let Some(info) = schema_info else {
        return;
    };

    let mem_before = get_memory_usage();
    let mut total_parse_time = Duration::ZERO;
    let mut total_validate_time = Duration::ZERO;
    let mut validation_errors = 0usize;

    // Parse only
    for _ in 0..iterations {
        let reader = BufReader::new(std::io::Cursor::new(content));
        let start = Instant::now();
        let mut parser = StreamingParser::new(reader);
        let handler = StatsHandler::new();
        parser.add_handler(Box::new(handler));
        let _ = parser.parse();
        total_parse_time += start.elapsed();
    }

    // Parse + validate
    for i in 0..iterations {
        let reader = BufReader::new(std::io::Cursor::new(content));
        let start = Instant::now();
        let mut parser = StreamingParser::new(reader);
        let handler = StatsHandler::new();
        parser.add_handler(Box::new(handler));
        let validator = OnePassSchemaValidator::new(Arc::clone(&info.compiled));
        parser.add_handler(Box::new(validator));
        let result = parser.parse();
        total_validate_time += start.elapsed();

        if i == 0 && result.is_ok() {
            let mut handlers = parser.into_handlers();
            if handlers.len() > 1
                && let Some(validator) = handlers
                    .pop()
                    .map(|h| h.as_any())
                    .and_then(|h| h.downcast::<OnePassSchemaValidator>().ok())
            {
                let errors = validator.into_errors();
                validation_errors = errors.iter().filter(|e| e.is_error()).count();
            }
        }
    }

    let mem_after = get_memory_usage();
    let avg_parse = total_parse_time / iterations as u32;
    let avg_validate = total_validate_time / iterations as u32;
    let parse_throughput = content.len() as f64 / avg_parse.as_secs_f64() / (1024.0 * 1024.0);
    let validate_throughput = content.len() as f64 / avg_validate.as_secs_f64() / (1024.0 * 1024.0);

    let memory_delta = match (mem_before, mem_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    output_json_result(&json!({
        "mode": "streaming-validate",
        "parse_time_ms": avg_parse.as_secs_f64() * 1000.0,
        "validate_time_ms": avg_validate.as_secs_f64() * 1000.0,
        "throughput_mb_s": parse_throughput,
        "validate_throughput_mb_s": validate_throughput,
        "memory_delta_bytes": memory_delta,
        "validation_errors": validation_errors,
    }));
}

/// Run transform benchmark in subprocess mode (outputs JSON)
fn run_transform_subprocess(content: &[u8], iterations: usize) {
    // Convert to string since StreamTransformer requires &str
    let input = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to convert to UTF-8: {}", e);
            return;
        }
    };

    let mem_before = get_memory_usage();
    let mut total_time = Duration::ZERO;
    let mut transform_count = 0usize;
    let mut output_size = 0usize;

    for i in 0..iterations {
        let start = Instant::now();

        // Use a simple identity transform on all elements (worst case for memory)
        let result = StreamTransformer::new(input)
            .with_root_namespaces()
            .ok()
            .map(|t| {
                t.on("//*", |_node| {
                    // no-op: just measure the overhead of building DOM subtrees
                })
                .run()
            });

        total_time += start.elapsed();

        if i == 0 {
            if let Some(Ok(output)) = result {
                transform_count = output.count();
                output_size = output.into_bytes().len();
            }
        }
    }

    let mem_after = get_memory_usage();
    let avg_time = total_time / iterations as u32;
    let throughput = content.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    let memory_delta = match (mem_before, mem_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    output_json_result(&json!({
        "mode": "transform",
        "parse_time_ms": avg_time.as_secs_f64() * 1000.0,
        "throughput_mb_s": throughput,
        "memory_delta_bytes": memory_delta,
        "transform_count": transform_count,
        "output_size": output_size,
    }));
}

/// Run reader-based transform benchmark in subprocess mode (outputs JSON)
///
/// Reads directly from the file via BufReader and writes to sink to
/// properly measure the memory-efficient reader-based approach.
fn run_transform_reader_subprocess(content: &[u8], iterations: usize, file_path: &str) {
    // Extract namespaces from the content (lightweight - only reads root element)
    let ns_map = match std::str::from_utf8(content) {
        Ok(s) => fastxml::namespace::extract_root_namespaces(s).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    };
    let file_size = content.len();

    let mem_before = get_memory_usage();
    let mut total_time = Duration::ZERO;
    let mut transform_count = 0usize;

    for i in 0..iterations {
        let start = Instant::now();

        // Read directly from file, write to sink - the true streaming path
        let file = std::fs::File::open(file_path).unwrap();
        let reader = std::io::BufReader::with_capacity(64 * 1024, file);

        let mut transformer = StreamTransformerReader::new(reader);
        for (prefix, uri) in &ns_map {
            transformer = transformer.namespace(prefix, uri);
        }

        let result = transformer
            .on("//*", |_node| {})
            .run_to_writer(&mut std::io::sink());

        total_time += start.elapsed();

        if i == 0 {
            if let Ok(count) = result {
                transform_count = count;
            }
        }
    }

    let mem_after = get_memory_usage();
    let avg_time = total_time / iterations as u32;
    let throughput = file_size as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    let memory_delta = match (mem_before, mem_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    };

    output_json_result(&json!({
        "mode": "transform-reader",
        "parse_time_ms": avg_time.as_secs_f64() * 1000.0,
        "throughput_mb_s": throughput,
        "memory_delta_bytes": memory_delta,
        "transform_count": transform_count,
    }));
}

// =============================================================================
// Normal Mode Benchmarks (print to console)
// =============================================================================

fn print_json_result(result: &JsonValue, prefix: &str) {
    let mode = result["mode"].as_str().unwrap_or("unknown");
    let time_ms = result["parse_time_ms"].as_f64().unwrap_or(0.0);
    let throughput = result["throughput_mb_s"].as_f64().unwrap_or(0.0);

    let time_str = if time_ms >= 1000.0 {
        format!("{:.2}s", time_ms / 1000.0)
    } else {
        format!("{:.0}ms", time_ms)
    };

    println!(
        "    {}{}: {} ({:.2} MB/s)",
        prefix, mode, time_str, throughput
    );

    if let Some(node_count) = result["node_count"].as_u64() {
        println!("    {} nodes: {}", prefix, node_count);
    }

    if let Some(mem) = result["memory_delta_bytes"].as_u64() {
        println!("    {} mem: Δ {}", prefix, format_bytes(mem as usize));
    }

    if let Some(transform_count) = result["transform_count"].as_u64() {
        println!("    {} transforms: {}", prefix, transform_count);
    }

    if let Some(output_size) = result["output_size"].as_u64() {
        println!(
            "    {} output: {}",
            prefix,
            format_bytes(output_size as usize)
        );
    }

    if let Some(errors) = result["validation_errors"].as_u64()
        && errors > 0
    {
        println!("    {} validation errors: {}", prefix, errors);
    }

    if let Some(validate_time_ms) = result["validate_time_ms"].as_f64() {
        let validate_str = if validate_time_ms >= 1000.0 {
            format!("{:.2}s", validate_time_ms / 1000.0)
        } else {
            format!("{:.0}ms", validate_time_ms)
        };
        if let Some(validate_throughput) = result["validate_throughput_mb_s"].as_f64() {
            println!(
                "    {} + validate: {} ({:.2} MB/s)",
                prefix, validate_str, validate_throughput
            );
        }
    }
}

fn run_dom_benchmark(content: &[u8], iterations: usize, schema_info: Option<&SchemaInfo>) {
    println!("\n  [DOM]");

    let mut total_parse_time = Duration::ZERO;
    let mut fastxml_mem_delta: Option<usize> = None;

    for i in 0..iterations {
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

    // fastxml DOM + validation benchmark
    if let Some(info) = schema_info {
        let ctx = XmlSchemaValidationContext::from_arc(Arc::clone(&info.compiled));
        let mut total_validate_time = Duration::ZERO;
        let mut fastxml_val_mem_delta: Option<usize> = None;
        let mut fastxml_validation_errors = 0usize;

        for i in 0..iterations {
            let mem_before = if i == 0 { get_memory_usage() } else { None };

            let start = Instant::now();
            let doc = parse(content).unwrap();
            let errors = ctx.validate(&doc).unwrap_or_default();
            total_validate_time += start.elapsed();

            if i == 0 {
                fastxml_validation_errors = errors.iter().filter(|e| e.is_error()).count();
                let mem_after = get_memory_usage();
                if let (Some(before), Some(after)) = (mem_before, mem_after) {
                    fastxml_val_mem_delta = Some(after.saturating_sub(before));
                }
            }
        }

        let avg_validate = total_validate_time / iterations as u32;
        let validate_throughput =
            content.len() as f64 / avg_validate.as_secs_f64() / (1024.0 * 1024.0);

        println!(
            "    fastxml + validate: {} ({:.2} MB/s)",
            format_duration(avg_validate),
            validate_throughput
        );
        if fastxml_validation_errors > 0 {
            println!(
                "    fastxml validation errors: {}",
                fastxml_validation_errors
            );
        }
        if let Some(mem) = fastxml_val_mem_delta {
            println!("    fastxml val mem: Δ {}", format_bytes(mem));
        }
    }

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

            println!();
            println!("    [Comparison]");
            let speedup = libxml_result.avg_time.as_secs_f64() / avg_parse.as_secs_f64();
            if speedup >= 1.0 {
                println!("    Speed:  fastxml is {:.2}x faster", speedup);
            } else {
                println!("    Speed:  libxml is {:.2}x faster", 1.0 / speedup);
            }

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

        if let Some(info) = schema_info
            && let (Some(export_dir), Some(entry_filename)) =
                (&info.export_dir, &info.entry_filename)
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

    let _ = schema_info;
}

fn run_streaming_benchmark(
    content: &[u8],
    iterations: usize,
    schema: Option<&Arc<CompiledSchema>>,
) {
    println!("\n  [Streaming]");
    let mem_before = get_memory_usage();

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

    if let Some(s) = schema {
        let mut total_validate_time = Duration::ZERO;
        let mut validation_errors = Vec::new();
        for i in 0..iterations {
            let reader = BufReader::new(std::io::Cursor::new(content));
            let start = Instant::now();
            let mut parser = StreamingParser::new(reader);
            let handler = StatsHandler::new();
            parser.add_handler(Box::new(handler));
            let validator = OnePassSchemaValidator::new(Arc::clone(s));
            parser.add_handler(Box::new(validator));
            let result = parser.parse();
            total_validate_time += start.elapsed();

            if i == 0 && result.is_ok() {
                let mut handlers = parser.into_handlers();
                if handlers.len() > 1
                    && let Some(validator) = handlers
                        .pop()
                        .map(|h| h.as_any())
                        .and_then(|h| h.downcast::<OnePassSchemaValidator>().ok())
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

        if !validation_errors.is_empty() {
            let error_count = validation_errors.iter().filter(|e| e.is_error()).count();
            let warning_count = validation_errors.iter().filter(|e| e.is_warning()).count();
            println!(
                "    Errors:     {} errors, {} warnings",
                error_count, warning_count
            );
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

fn run_transform_benchmark(content: &[u8], iterations: usize) {
    println!("\n  [Transform]");
    let input = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(e) => {
            println!("    Skipped: UTF-8 error: {}", e);
            return;
        }
    };

    let mem_before = get_memory_usage();
    let mut total_time = Duration::ZERO;
    let mut transform_count = 0usize;

    for i in 0..iterations {
        let start = Instant::now();

        let result = StreamTransformer::new(input)
            .with_root_namespaces()
            .ok()
            .map(|t| {
                t.on("//*", |_node| {
                    // no-op: measure overhead of building DOM subtrees
                })
                .run()
            });

        total_time += start.elapsed();

        if i == 0 {
            if let Some(Ok(ref output)) = result {
                transform_count = output.count();
            }
        }
    }

    let mem_after = get_memory_usage();
    let avg_time = total_time / iterations as u32;
    let throughput = content.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    println!(
        "    Time:       {} ({:.2} MB/s)",
        format_duration(avg_time),
        throughput
    );
    println!("    Transforms: {}", transform_count);

    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        println!(
            "    Memory:     Δ {}",
            format_bytes(after.saturating_sub(before))
        );
    }
}

fn run_transform_reader_benchmark(content: &[u8], iterations: usize) {
    println!("\n  [Transform Reader]");
    let ns_map = match std::str::from_utf8(content) {
        Ok(s) => fastxml::namespace::extract_root_namespaces(s).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    };

    let mem_before = get_memory_usage();
    let mut total_time = Duration::ZERO;
    let mut transform_count = 0usize;

    for i in 0..iterations {
        let start = Instant::now();

        let reader = std::io::BufReader::with_capacity(64 * 1024, std::io::Cursor::new(content));
        let mut output = Vec::new();

        let mut transformer = StreamTransformerReader::new(reader);
        for (prefix, uri) in &ns_map {
            transformer = transformer.namespace(prefix, uri);
        }

        let result = transformer.on("//*", |_node| {}).run_to_writer(&mut output);

        total_time += start.elapsed();

        if i == 0 && let Ok(count) = result {
            transform_count = count;
        }
    let mem_after = get_memory_usage();
    let avg_time = total_time / iterations as u32;
    let throughput = content.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    println!(
        "    Time:       {} ({:.2} MB/s)",
        format_duration(avg_time),
        throughput
    );
    println!("    Transforms: {}", transform_count);

    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        println!(
            "    Memory:     Δ {}",
            format_bytes(after.saturating_sub(before))
        );
    }
}

fn run_pattern_test(config: GeneratorConfig, mode: ProcessingMode, iterations: usize) {
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

    let xml_bytes = if mode == ProcessingMode::Streaming {
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

    if mode == ProcessingMode::Dom || mode == ProcessingMode::Both {
        run_dom_benchmark(&xml_bytes, iterations, None);

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

    if mode == ProcessingMode::Streaming || mode == ProcessingMode::Both {
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

/// Run file benchmark with subprocess isolation for mode=Both
#[allow(clippy::too_many_arguments)]
fn run_file_benchmark(
    name: &str,
    file_path: Option<&str>,
    content: &[u8],
    mode: ProcessingMode,
    iterations: usize,
    validate: bool,
    inputs: &[String],
    cache_dir: &Path,
) {
    println!("\n--- {} ({}) ---", name, format_bytes(content.len()));

    // For mode=Both, we could use subprocesses for isolation
    // But for simplicity and backward compatibility, we run directly here
    // The subprocess isolation is used when --__internal is specified

    let schema_info: Option<SchemaInfo> = if validate {
        get_schema_from_content(content, file_path)
    } else {
        None
    };

    match mode {
        ProcessingMode::Both => {
            // Run DOM in subprocess for clean memory measurement
            println!("\n  [DOM] (subprocess)");
            if let Some(result) = run_subprocess(InternalMode::Dom, inputs, iterations, cache_dir) {
                print_json_result(&result, "");
            } else {
                // Fallback to direct execution
                run_dom_benchmark(content, iterations, schema_info.as_ref());
            }

            // Run Streaming in subprocess for clean memory measurement
            println!("\n  [Streaming] (subprocess)");
            if let Some(result) =
                run_subprocess(InternalMode::Streaming, inputs, iterations, cache_dir)
            {
                print_json_result(&result, "");
            } else {
                // Fallback to direct execution
                run_streaming_benchmark(
                    content,
                    iterations,
                    schema_info.as_ref().map(|s| &s.compiled),
                );
            }

            // Run Transform in subprocess for clean memory measurement
            println!("\n  [Transform] (subprocess)");
            if let Some(result) =
                run_subprocess(InternalMode::Transform, inputs, iterations, cache_dir)
            {
                print_json_result(&result, "");
            } else {
                run_transform_benchmark(content, iterations);
            }

            // Run Transform Reader in subprocess for clean memory measurement
            println!("\n  [Transform Reader] (subprocess)");
            if let Some(result) =
                run_subprocess(InternalMode::TransformReader, inputs, iterations, cache_dir)
            {
                print_json_result(&result, "");
            } else {
                run_transform_reader_benchmark(content, iterations);
            }

            // Validation benchmarks
            if validate {
                println!("\n  [DOM + Validate] (subprocess)");
                if let Some(result) =
                    run_subprocess(InternalMode::DomValidate, inputs, iterations, cache_dir)
                {
                    print_json_result(&result, "");
                }

                println!("\n  [Streaming + Validate] (subprocess)");
                if let Some(result) = run_subprocess(
                    InternalMode::StreamingValidate,
                    inputs,
                    iterations,
                    cache_dir,
                ) {
                    print_json_result(&result, "");
                }

                // libxml comparison in subprocess
                #[cfg(feature = "compare-libxml")]
                {
                    println!("\n  [libxml] (subprocess)");
                    if let Some(result) =
                        run_subprocess(InternalMode::Libxml, inputs, iterations, cache_dir)
                    {
                        print_json_result(&result, "");
                    }

                    println!("\n  [libxml + Validate] (subprocess)");
                    if let Some(result) =
                        run_subprocess(InternalMode::LibxmlValidate, inputs, iterations, cache_dir)
                    {
                        print_json_result(&result, "");
                    }
                }
            }
        }
        ProcessingMode::Dom => {
            run_dom_benchmark(content, iterations, schema_info.as_ref());
        }
        ProcessingMode::Streaming => {
            run_streaming_benchmark(
                content,
                iterations,
                schema_info.as_ref().map(|s| &s.compiled),
            );
        }
        ProcessingMode::Transform => {
            run_transform_benchmark(content, iterations);
        }
        ProcessingMode::TransformReader => {
            run_transform_reader_benchmark(content, iterations);
        }
    }
}

// =============================================================================
// Main
// =============================================================================

fn main() {
    let args = Args::parse();

    // Handle internal subprocess mode
    if let Some(internal_mode) = args.internal_mode {
        // Load files for subprocess
        let inputs = if args.inputs.is_empty() && !std::io::stdin().is_terminal() {
            let stdin = std::io::stdin();
            stdin
                .lock()
                .lines()
                .map_while(|l| l.ok())
                .filter(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with('#')
                })
                .collect()
        } else {
            args.inputs.clone()
        };

        if inputs.is_empty() {
            eprintln!("No input files for subprocess");
            std::process::exit(1);
        }

        // Load first file
        let input = &inputs[0];
        let content = match load_file(input, &args.cache_dir) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to load file: {}", e);
                std::process::exit(1);
            }
        };

        let file_path = if is_url(input) {
            None
        } else {
            Some(input.as_str())
        };

        match internal_mode {
            InternalMode::Dom => run_dom_subprocess(&content, args.iterations),
            InternalMode::DomValidate => {
                run_dom_validate_subprocess(&content, args.iterations, file_path)
            }
            InternalMode::Streaming => run_streaming_subprocess(&content, args.iterations),
            InternalMode::StreamingValidate => {
                run_streaming_validate_subprocess(&content, args.iterations, file_path)
            }
            InternalMode::Transform => run_transform_subprocess(&content, args.iterations),
            InternalMode::TransformReader => {
                run_transform_reader_subprocess(&content, args.iterations, file_path.unwrap_or(""))
            }
            #[cfg(feature = "compare-libxml")]
            InternalMode::Libxml => libxml_bench::run_libxml_subprocess(&content, args.iterations),
            #[cfg(feature = "compare-libxml")]
            InternalMode::LibxmlValidate => {
                // Need schema path for libxml validation
                if let Some(schema_info) = get_schema_from_content(&content, file_path)
                    && let (Some(export_dir), Some(entry_filename)) =
                        (schema_info.export_dir, schema_info.entry_filename)
                {
                    let schema_path = export_dir.join(&entry_filename);
                    libxml_bench::run_libxml_validate_subprocess(
                        &content,
                        &schema_path,
                        args.iterations,
                    );
                }
            }
        }

        return;
    }

    // Read from stdin if no inputs and not a terminal
    let inputs =
        if args.inputs.is_empty() && args.pattern.is_none() && !std::io::stdin().is_terminal() {
            let stdin = std::io::stdin();
            stdin
                .lock()
                .lines()
                .map_while(|l| l.ok())
                .filter(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with('#')
                })
                .collect()
        } else {
            args.inputs.clone()
        };

    println!();
    print_separator();
    println!("  fastxml Benchmark CLI");
    print_separator();

    if let Some(pattern) = args.pattern {
        println!("Mode: Synthetic ({:?})", pattern);
        println!(
            "Processing: {:?}, Iterations: {}",
            args.mode, args.iterations
        );

        let gen_config = match pattern {
            Pattern::ManyElements => GeneratorConfig::many_elements(args.size),
            Pattern::DeepNesting => GeneratorConfig::deep_nesting(args.size),
            Pattern::LargeContent => GeneratorConfig::large_content(args.size * 1024),
            Pattern::Citygml => GeneratorConfig::citygml_style(args.size),
        };

        run_pattern_test(gen_config, args.mode, args.iterations);
    } else if !inputs.is_empty() {
        println!("Mode: Real files ({} inputs)", inputs.len());
        println!(
            "Processing: {:?}, Iterations: {}, Validate: {}",
            args.mode, args.iterations, args.validate
        );

        println!("\n--- Loading Files ---");
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut total_size = 0usize;

        for input in &inputs {
            match load_file(input, &args.cache_dir) {
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

        for (input, content) in &files {
            let file_path = if is_url(input) {
                None
            } else {
                Some(input.as_str())
            };
            run_file_benchmark(
                get_display_name(input),
                file_path,
                content,
                args.mode,
                args.iterations,
                args.validate,
                &inputs,
                &args.cache_dir,
            );
        }

        if files.len() > 1 {
            println!();
            print_separator();
            println!("  Summary");
            print_separator();
            println!("  Files:      {}", files.len());
            println!("  Total size: {}", format_bytes(total_size));
        }
    } else {
        // Default to pattern mode
        println!("Mode: Synthetic (ManyElements)");
        println!(
            "Processing: {:?}, Iterations: {}",
            args.mode, args.iterations
        );

        let gen_config = GeneratorConfig::many_elements(args.size);
        run_pattern_test(gen_config, args.mode, args.iterations);
    }

    println!();
    print_separator();
    println!("Done!");
}
