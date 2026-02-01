//! Load testing CLI for fastxml.
//!
//! This tool generates large XML documents and measures parsing performance.
//!
//! Usage:
//!   cargo run --release --example load_test_cli -- [OPTIONS]
//!
//! Options:
//!   --pattern <PATTERN>   Test pattern: many-elements, deep-nesting, large-content, citygml
//!   --size <SIZE>         Size parameter (element count, depth, or content size in KB)
//!   --mode <MODE>         Processing mode: dom, streaming, both
//!   --iterations <N>      Number of iterations for timing

use std::io::{BufRead, BufReader, Read};
use std::time::{Duration, Instant};

use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::generator::{GeneratorConfig, XmlStreamGenerator};
use fastxml::{evaluate, parse};

/// Handler that collects statistics during streaming parse.
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

    #[allow(dead_code)]
    fn report(&self) {
        println!("  Elements:    {:>10}", self.element_count);
        println!("  Attributes:  {:>10}", self.attr_count);
        println!("  Max Depth:   {:>10}", self.max_depth);
        println!("  Text Bytes:  {:>10}", format_bytes(self.text_bytes));
    }
}

impl XmlEventHandler for StatsHandler {
    fn handle(&mut self, event: &XmlEvent) -> fastxml::error::Result<()> {
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
}

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
        Some(rss * 1024) // ps reports in KB
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn run_test(config: GeneratorConfig, mode: &str, iterations: usize) {
    println!("\n{}", "=".repeat(60));
    println!("Configuration:");
    println!("  Elements:     {:>10}", config.element_count);
    println!("  Max Depth:    {:>10}", config.max_depth);
    println!("  Content Size: {:>10}", format_bytes(config.content_size));
    println!("  Attributes:   {:>10}/element", config.attribute_count);
    println!("  Namespaces:   {:>10}", config.with_namespaces);
    println!("  Est. Size:    {:>10}", format_bytes(config.estimated_size()));
    println!("{}", "=".repeat(60));

    // Generate XML once for DOM tests
    let xml_bytes = if mode == "streaming" {
        Vec::new()
    } else {
        println!("\nGenerating XML...");
        let start = Instant::now();
        let mut xml_gen = XmlStreamGenerator::new(config.clone());
        let mut bytes = Vec::new();
        xml_gen.read_to_end(&mut bytes).unwrap();
        println!("  Generated {} in {}", format_bytes(bytes.len()), format_duration(start.elapsed()));
        bytes
    };

    // DOM parsing test
    if mode == "dom" || mode == "both" {
        println!("\n--- DOM Parsing ---");
        let mem_before = get_memory_usage();

        let mut total_time = Duration::ZERO;
        let mut node_count = 0;

        for i in 0..iterations {
            let start = Instant::now();
            let doc = parse(&xml_bytes).unwrap();
            node_count = doc.node_count();
            total_time += start.elapsed();

            if i == 0 {
                println!("  Nodes: {}", node_count);
            }
        }

        let mem_after = get_memory_usage();

        let avg_time = total_time / iterations as u32;
        let throughput = xml_bytes.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

        println!("  Avg Time:    {}", format_duration(avg_time));
        println!("  Throughput:  {:.2} MB/s", throughput);
        println!("  Elements/s:  {:.0}", node_count as f64 / avg_time.as_secs_f64());

        if let (Some(before), Some(after)) = (mem_before, mem_after) {
            println!("  Memory:      {} -> {} (Δ {})",
                format_bytes(before),
                format_bytes(after),
                format_bytes(after.saturating_sub(before)));
        }

        // XPath test
        println!("\n--- XPath Evaluation ---");
        let doc = parse(&xml_bytes).unwrap();

        let start = Instant::now();
        let result = evaluate(&doc, "//*").unwrap();
        let count = result.into_nodes().len();
        println!("  //*: {} elements in {}", count, format_duration(start.elapsed()));

        if config.with_namespaces {
            let start = Instant::now();
            let result = evaluate(&doc, "//bldg:Building").unwrap();
            let count = result.into_nodes().len();
            println!("  //bldg:Building: {} elements in {}", count, format_duration(start.elapsed()));
        }
    }

    // Streaming test
    if mode == "streaming" || mode == "both" {
        println!("\n--- Streaming Parse ---");
        let mem_before = get_memory_usage();

        let mut total_time = Duration::ZERO;
        let mut total_bytes = 0usize;

        for i in 0..iterations {
            let xml_gen = XmlStreamGenerator::new(config.clone());
            let reader = BufReader::with_capacity(64 * 1024, xml_gen);

            let start = Instant::now();

            // Count bytes while reading
            let mut counting_reader = CountingReader::new(reader);
            let mut parser = StreamingParser::new(&mut counting_reader);
            let handler = StatsHandler::new();
            parser.add_handler(Box::new(handler));
            parser.parse().unwrap();

            total_time += start.elapsed();
            total_bytes = counting_reader.bytes_read;

            if i == 0 {
                // Can't easily get handler stats back, just show bytes
                println!("  Processed: {}", format_bytes(total_bytes));
            }
        }

        let mem_after = get_memory_usage();

        let avg_time = total_time / iterations as u32;
        let throughput = total_bytes as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

        println!("  Avg Time:    {}", format_duration(avg_time));
        println!("  Throughput:  {:.2} MB/s", throughput);

        if let (Some(before), Some(after)) = (mem_before, mem_after) {
            println!("  Memory:      {} -> {} (Δ {})",
                format_bytes(before),
                format_bytes(after),
                format_bytes(after.saturating_sub(before)));
        }
    }
}

struct CountingReader<R> {
    inner: R,
    bytes_read: usize,
}

impl<R> CountingReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, bytes_read: 0 }
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

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut pattern = "many-elements";
    let mut size = 10_000usize;
    let mut mode = "both";
    let mut iterations = 3usize;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pattern" => {
                i += 1;
                pattern = &args[i];
            }
            "--size" => {
                i += 1;
                size = args[i].parse().unwrap_or(10_000);
            }
            "--mode" => {
                i += 1;
                mode = &args[i];
            }
            "--iterations" => {
                i += 1;
                iterations = args[i].parse().unwrap_or(3);
            }
            "--help" | "-h" => {
                println!("fastxml Load Test CLI");
                println!();
                println!("Usage: load_test_cli [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --pattern <PATTERN>   Test pattern:");
                println!("                        - many-elements (default)");
                println!("                        - deep-nesting");
                println!("                        - large-content");
                println!("                        - citygml");
                println!("  --size <SIZE>         Size parameter (default: 10000)");
                println!("                        - many-elements: element count");
                println!("                        - deep-nesting: depth");
                println!("                        - large-content: KB per element");
                println!("                        - citygml: building count");
                println!("  --mode <MODE>         Processing mode: dom, streaming, both (default)");
                println!("  --iterations <N>      Number of iterations (default: 3)");
                return;
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
            }
        }
        i += 1;
    }

    println!("fastxml Load Test");
    println!("Pattern: {}, Size: {}, Mode: {}, Iterations: {}", pattern, size, mode, iterations);

    let config = match pattern {
        "many-elements" => GeneratorConfig::many_elements(size),
        "deep-nesting" => GeneratorConfig::deep_nesting(size),
        "large-content" => GeneratorConfig::large_content(size * 1024),
        "citygml" => GeneratorConfig::citygml_style(size),
        _ => {
            eprintln!("Unknown pattern: {}", pattern);
            return;
        }
    };

    run_test(config, mode, iterations);

    println!("\n{}", "=".repeat(60));
    println!("Done!");
}
