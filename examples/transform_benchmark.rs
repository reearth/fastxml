//! Benchmark demonstrating memory efficiency of streaming XML transformation.
//!
//! Usage:
//!   cargo run --release --example transform_benchmark
//!   cargo run --release --features profile --example transform_benchmark  # with memory stats
//!
//! This example shows how the transform module processes large XML files
//! efficiently by only building DOM for matched elements.

use std::io::Read;
use std::time::Instant;

use fastxml::generator::{GeneratorConfig, XmlStreamGenerator};
use fastxml::profile::get_memory_usage;
use fastxml::transform::StreamTransformer;

fn main() {
    println!("=== Streaming XML Transform Benchmark ===\n");

    #[cfg(feature = "profile")]
    println!("Memory profiling: ENABLED\n");
    #[cfg(not(feature = "profile"))]
    println!("Memory profiling: disabled (enable with --features profile)\n");

    // Test with different sizes
    for &element_count in &[1_000, 10_000, 100_000, 1_000_000] {
        run_benchmark(element_count);
        println!();
    }
}

fn format_memory(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.2} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

fn run_benchmark(element_count: usize) {
    println!("--- {} elements ---", element_count);

    // Generate XML
    let config = GeneratorConfig::many_elements(element_count);
    let estimated_size = config.estimated_size();
    println!(
        "Generating XML (~{:.1} MB estimated)...",
        estimated_size as f64 / 1_000_000.0
    );

    let gen_start = Instant::now();
    let mut generator = XmlStreamGenerator::new(config);
    let mut xml = String::new();
    generator.read_to_string(&mut xml).unwrap();
    let gen_time = gen_start.elapsed();

    let actual_size = xml.len();
    println!(
        "  Generated: {:.2} MB in {:?}",
        actual_size as f64 / 1_000_000.0,
        gen_time
    );

    // =========================================================================
    // Benchmark 1: Streaming Transform (ALL 'item' elements)
    // =========================================================================
    println!("\n[1] Streaming Transform - ALL 'item' nodes:");
    let mem_before = get_memory_usage();
    let start = Instant::now();
    let output1 = StreamTransformer::new(&xml)
        .on("//item", |node| {
            node.set_attribute("transformed", "true");
        })
        .run()
        .unwrap();
    let count1 = output1.count();
    let output1_bytes = output1.into_bytes();
    let time1 = start.elapsed();
    let mem_after = get_memory_usage();

    println!("  Matched: {} nodes", count1);
    println!("  Time: {:?}", time1);
    println!(
        "  Throughput: {:.1} MB/s",
        actual_size as f64 / 1_000_000.0 / time1.as_secs_f64()
    );
    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        let mem_used = after.saturating_sub(before);
        println!("  Memory delta: +{}", format_memory(mem_used));
    }
    drop(output1_bytes);

    // =========================================================================
    // Benchmark 2: Streaming Transform (selective with attribute predicate)
    // =========================================================================
    println!("\n[2] Streaming Transform - 'item' nodes with attr0='value1':");
    let mem_before = get_memory_usage();
    let start = Instant::now();
    let output2 = StreamTransformer::new(&xml)
        .on("//item[@attr0='value1']", |node| {
            node.set_attribute("special", "true");
        })
        .run()
        .unwrap();
    let count2 = output2.count();
    let output2_bytes = output2.into_bytes();
    let time2 = start.elapsed();
    let mem_after = get_memory_usage();

    println!("  Matched: {} nodes", count2);
    println!("  Time: {:?}", time2);
    println!(
        "  Throughput: {:.1} MB/s",
        actual_size as f64 / 1_000_000.0 / time2.as_secs_f64()
    );
    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        let mem_used = after.saturating_sub(before);
        println!("  Memory delta: +{}", format_memory(mem_used));
    }
    drop(output2_bytes);

    // =========================================================================
    // Benchmark 3: Streaming Remove
    // =========================================================================
    println!("\n[3] Streaming Transform - Remove all 'data' nodes:");
    let mem_before = get_memory_usage();
    let start = Instant::now();
    let output3 = StreamTransformer::new(&xml)
        .on("//data", |node| {
            node.remove();
        })
        .run()
        .unwrap();
    let count3 = output3.count();
    let output3_bytes = output3.into_bytes();
    let time3 = start.elapsed();
    let mem_after = get_memory_usage();

    println!("  Removed: {} nodes", count3);
    println!("  Time: {:?}", time3);
    println!("  Input:  {:.2} MB", actual_size as f64 / 1_000_000.0);
    println!(
        "  Output: {:.2} MB ({:.1}% reduction)",
        output3_bytes.len() as f64 / 1_000_000.0,
        (1.0 - output3_bytes.len() as f64 / actual_size as f64) * 100.0
    );
    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        let mem_used = after.saturating_sub(before);
        println!("  Memory delta: +{}", format_memory(mem_used));
    }

    // =========================================================================
    // Benchmark 4: Full DOM Parse + XPath (comparison)
    // =========================================================================
    println!("\n[4] Full DOM Parse + XPath (same query as [2]):");
    let mem_before = get_memory_usage();
    let start = Instant::now();
    let doc = fastxml::parse(&xml).unwrap();
    let parse_time = start.elapsed();
    let mem_after_parse = get_memory_usage();

    let start = Instant::now();
    let result = fastxml::xpath::evaluate(&doc, "//item[@attr0='value1']").unwrap();
    let xpath_time = start.elapsed();
    let mem_after = get_memory_usage();

    let matched_count = match result {
        fastxml::xpath::XPathResult::Nodes(nodes) => nodes.len(),
        _ => 0,
    };

    println!("  DOM parse: {:?}", parse_time);
    println!("  XPath eval: {:?}", xpath_time);
    println!("  Total: {:?}", parse_time + xpath_time);
    println!("  Matched: {} nodes", matched_count);

    if let (Some(before), Some(after_parse)) = (mem_before, mem_after_parse) {
        let dom_mem = after_parse.saturating_sub(before);
        println!("  DOM memory: +{}", format_memory(dom_mem));
    }
    if let (Some(before), Some(after)) = (mem_before, mem_after) {
        let total_mem = after.saturating_sub(before);
        println!("  Total memory: +{}", format_memory(total_mem));
    }

    // =========================================================================
    // Summary comparison
    // =========================================================================
    println!("\n[Summary]");
    println!(
        "  Streaming transform was {:.1}x faster than DOM+XPath",
        (parse_time + xpath_time).as_secs_f64() / time2.as_secs_f64()
    );
}
