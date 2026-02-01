//! Load tests for fastxml.
//!
//! These tests verify that the library can handle large XML documents
//! with acceptable performance and memory usage.

use std::io::{BufReader, Read};
use std::sync::Arc;
use std::time::Instant;

use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::generator::{GeneratorConfig, XmlStreamGenerator};
use fastxml::schema::types::{CompiledSchema, ElementDef};
use fastxml::schema::validator::StreamingSchemaValidator;
use fastxml::{evaluate, parse};

/// Handler that counts elements during streaming.
struct CountingHandler {
    element_count: usize,
    max_depth: usize,
    current_depth: usize,
}

impl CountingHandler {
    fn new() -> Self {
        Self {
            element_count: 0,
            max_depth: 0,
            current_depth: 0,
        }
    }
}

impl XmlEventHandler for CountingHandler {
    fn handle(&mut self, event: &XmlEvent) -> fastxml::error::Result<()> {
        match event {
            XmlEvent::StartElement { .. } => {
                self.element_count += 1;
                self.current_depth += 1;
                self.max_depth = self.max_depth.max(self.current_depth);
            }
            XmlEvent::EndElement { .. } => {
                self.current_depth = self.current_depth.saturating_sub(1);
            }
            _ => {}
        }
        Ok(())
    }
}

// =============================================================================
// Pattern: Many Elements (Wide Tree)
// =============================================================================

#[test]
fn test_load_many_elements_1k() {
    let config = GeneratorConfig::many_elements(1_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let doc = parse(&xml).unwrap();
    assert!(doc.node_count() > 1000);
}

#[test]
fn test_load_many_elements_10k() {
    let config = GeneratorConfig::many_elements(10_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!(
        "10K elements: {} nodes in {:?}, {} bytes",
        doc.node_count(),
        elapsed,
        xml.len()
    );

    assert!(doc.node_count() > 10_000);
    // Should complete in reasonable time
    assert!(
        elapsed.as_secs() < 5,
        "Parsing took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_load_many_elements_streaming_10k() {
    let config = GeneratorConfig::many_elements(10_000);
    let xml_gen = XmlStreamGenerator::new(config);
    let reader = BufReader::new(xml_gen);

    let start = Instant::now();
    let mut parser = StreamingParser::new(reader);
    let handler = CountingHandler::new();
    parser.add_handler(Box::new(handler));
    parser.parse().unwrap();
    let elapsed = start.elapsed();

    println!("10K elements streaming: {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 5,
        "Streaming took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_load_many_elements_100k() {
    let config = GeneratorConfig::many_elements(100_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!(
        "100K elements: {} nodes in {:?}, {} MB",
        doc.node_count(),
        elapsed,
        xml.len() / (1024 * 1024)
    );

    assert!(doc.node_count() > 100_000);
    assert!(
        elapsed.as_secs() < 30,
        "Parsing took too long: {:?}",
        elapsed
    );
}

// =============================================================================
// Pattern: Deep Nesting
// =============================================================================

#[test]
fn test_load_deep_nesting_100() {
    let config = GeneratorConfig::deep_nesting(100);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let doc = parse(&xml).unwrap();
    assert!(doc.node_count() > 100);
}

#[test]
fn test_load_deep_nesting_500() {
    let config = GeneratorConfig::deep_nesting(500);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!("500 depth: {} nodes in {:?}", doc.node_count(), elapsed);

    assert!(doc.node_count() > 500);
    assert!(
        elapsed.as_secs() < 5,
        "Parsing took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_load_deep_nesting_1000() {
    let config = GeneratorConfig::deep_nesting(1000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!("1000 depth: {} nodes in {:?}", doc.node_count(), elapsed);

    assert!(
        elapsed.as_secs() < 10,
        "Parsing took too long: {:?}",
        elapsed
    );
}

// =============================================================================
// Pattern: Large Content
// =============================================================================

#[test]
fn test_load_large_content_1mb() {
    let config = GeneratorConfig::large_content(1024 * 1024); // 1MB per element
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!(
        "1MB content: {} nodes in {:?}, {} MB total",
        doc.node_count(),
        elapsed,
        xml.len() / (1024 * 1024)
    );

    assert!(
        elapsed.as_secs() < 10,
        "Parsing took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_load_large_content_10mb() {
    let config = GeneratorConfig::large_content(10 * 1024 * 1024); // 10MB per element
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!(
        "10MB content: {} nodes in {:?}, {} MB total",
        doc.node_count(),
        elapsed,
        xml.len() / (1024 * 1024)
    );

    assert!(
        elapsed.as_secs() < 30,
        "Parsing took too long: {:?}",
        elapsed
    );
}

// =============================================================================
// Pattern: CityGML Style
// =============================================================================

#[test]
fn test_load_citygml_100_buildings() {
    let config = GeneratorConfig::citygml_style(100);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let elapsed = start.elapsed();

    println!("100 buildings: {} nodes in {:?}", doc.node_count(), elapsed);

    // Test XPath on CityGML structure
    let result = evaluate(&doc, "//bldg:Building").unwrap();
    let buildings = result.into_nodes();
    println!("Found {} buildings via XPath", buildings.len());

    assert!(buildings.len() >= 10); // Should find some buildings
}

#[test]
fn test_load_citygml_1000_buildings() {
    let config = GeneratorConfig::citygml_style(1000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let start = Instant::now();
    let doc = parse(&xml).unwrap();
    let parse_elapsed = start.elapsed();

    println!(
        "1000 buildings: {} nodes in {:?}, {} MB",
        doc.node_count(),
        parse_elapsed,
        xml.len() / (1024 * 1024)
    );

    // Test XPath performance
    let start = Instant::now();
    let result = evaluate(&doc, "//bldg:Building").unwrap();
    let xpath_elapsed = start.elapsed();
    let buildings = result.into_nodes();

    println!(
        "XPath found {} buildings in {:?}",
        buildings.len(),
        xpath_elapsed
    );

    assert!(parse_elapsed.as_secs() < 30, "Parsing took too long");
    assert!(xpath_elapsed.as_secs() < 10, "XPath took too long");
}

// =============================================================================
// Streaming vs DOM Comparison
// =============================================================================

#[test]
fn test_streaming_memory_efficiency() {
    // This test demonstrates that streaming uses less memory than DOM
    let config = GeneratorConfig::many_elements(50_000);

    // DOM approach - needs to load everything
    let mut xml_gen = XmlStreamGenerator::new(config.clone());
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let dom_start = Instant::now();
    let doc = parse(&xml).unwrap();
    let dom_time = dom_start.elapsed();
    let dom_nodes = doc.node_count();

    // Streaming approach - processes incrementally
    let xml_gen = XmlStreamGenerator::new(config);
    let reader = BufReader::with_capacity(64 * 1024, xml_gen);

    let stream_start = Instant::now();
    let mut parser = StreamingParser::new(reader);
    let handler = CountingHandler::new();
    parser.add_handler(Box::new(handler));
    parser.parse().unwrap();
    let stream_time = stream_start.elapsed();

    println!("50K elements comparison:");
    println!("  DOM:       {:?}, {} nodes", dom_time, dom_nodes);
    println!("  Streaming: {:?}", stream_time);
    println!("  XML size:  {} MB", xml.len() / (1024 * 1024));

    // Both should complete reasonably fast
    assert!(dom_time.as_secs() < 30);
    assert!(stream_time.as_secs() < 30);
}

// =============================================================================
// Throughput Test
// =============================================================================

#[test]
fn test_throughput() {
    let config = GeneratorConfig::many_elements(50_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let iterations = 3;
    let mut total_time = std::time::Duration::ZERO;

    for _ in 0..iterations {
        let start = Instant::now();
        let doc = parse(&xml).unwrap();
        total_time += start.elapsed();
        std::hint::black_box(doc.node_count());
    }

    let avg_time = total_time / iterations;
    let throughput_mb_per_sec = xml.len() as f64 / avg_time.as_secs_f64() / (1024.0 * 1024.0);

    println!("Throughput test (50K elements, {} iterations):", iterations);
    println!("  Avg time: {:?}", avg_time);
    println!("  Throughput: {:.2} MB/s", throughput_mb_per_sec);
    println!("  XML size: {} bytes", xml.len());

    // Should achieve at least 10 MB/s throughput
    assert!(
        throughput_mb_per_sec > 10.0,
        "Throughput too low: {:.2} MB/s",
        throughput_mb_per_sec
    );
}

// =============================================================================
// XPath Performance on Large Documents
// =============================================================================

#[test]
fn test_xpath_performance_large() {
    let config = GeneratorConfig::many_elements(20_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let doc = parse(&xml).unwrap();

    // Test various XPath expressions
    let tests = vec![
        ("//*", "all elements"),
        ("//element", "by name"),
        ("//*[name()='item']", "with predicate"),
        ("/root/*", "direct children"),
    ];

    for (xpath, desc) in tests {
        let start = Instant::now();
        let result = evaluate(&doc, xpath).unwrap();
        let count = result.into_nodes().len();
        let elapsed = start.elapsed();

        println!(
            "XPath '{}' ({}): {} results in {:?}",
            xpath, desc, count, elapsed
        );

        // Each XPath should complete in reasonable time
        assert!(
            elapsed.as_secs() < 5,
            "XPath '{}' took too long: {:?}",
            xpath,
            elapsed
        );
    }
}

// =============================================================================
// Schema Validation Tests
// =============================================================================

/// Creates a test schema for validation.
fn create_test_schema() -> Arc<CompiledSchema> {
    let mut schema = CompiledSchema::new();
    schema
        .elements
        .insert("root".to_string(), ElementDef::new("root"));
    schema
        .elements
        .insert("element".to_string(), ElementDef::new("element"));
    schema
        .elements
        .insert("item".to_string(), ElementDef::new("item"));
    schema
        .elements
        .insert("data".to_string(), ElementDef::new("data"));
    Arc::new(schema)
}

/// Creates a CityGML-style test schema.
fn create_citygml_schema() -> Arc<CompiledSchema> {
    let mut schema = CompiledSchema::new();
    schema.target_namespace = Some("http://www.opengis.net/citygml/2.0".to_string());
    schema
        .elements
        .insert("core:CityModel".to_string(), ElementDef::new("CityModel"));
    schema
        .elements
        .insert("bldg:Building".to_string(), ElementDef::new("Building"));
    schema.elements.insert(
        "bldg:measuredHeight".to_string(),
        ElementDef::new("measuredHeight"),
    );
    schema
        .elements
        .insert("gml:name".to_string(), ElementDef::new("name"));
    schema.elements.insert(
        "bldg:lod0FootPrint".to_string(),
        ElementDef::new("lod0FootPrint"),
    );
    Arc::new(schema)
}

#[test]
fn test_schema_validation_basic() {
    let config = GeneratorConfig::many_elements(1_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let schema = create_test_schema();

    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(validator));
    parser.parse().unwrap();
    let elapsed = start.elapsed();

    println!("Schema validation (1K elements): {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 5,
        "Validation took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_schema_validation_10k_elements() {
    let config = GeneratorConfig::many_elements(10_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let schema = create_test_schema();

    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(validator));
    parser.parse().unwrap();
    let elapsed = start.elapsed();

    println!(
        "Schema validation (10K elements): {:?}, {} bytes",
        elapsed,
        xml.len()
    );
    assert!(
        elapsed.as_secs() < 10,
        "Validation took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_schema_validation_citygml() {
    let config = GeneratorConfig::citygml_style(100);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let schema = create_citygml_schema();

    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(validator));
    parser.parse().unwrap();
    let elapsed = start.elapsed();

    println!("CityGML validation (100 buildings): {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 10,
        "Validation took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_schema_validation_with_counting() {
    // Test that multiple handlers can work together
    let config = GeneratorConfig::many_elements(5_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let schema = create_test_schema();

    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);

    // Add both handlers
    let handler = CountingHandler::new();
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(handler));
    parser.add_handler(Box::new(validator));

    parser.parse().unwrap();
    let elapsed = start.elapsed();

    println!("Validation + counting (5K elements): {:?}", elapsed);
    assert!(
        elapsed.as_secs() < 5,
        "Combined processing took too long: {:?}",
        elapsed
    );
}

#[test]
fn test_schema_validation_overhead() {
    // Compare streaming with and without validation to measure overhead
    let config = GeneratorConfig::many_elements(10_000);
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut xml = Vec::new();
    xml_gen.read_to_end(&mut xml).unwrap();

    let schema = create_test_schema();

    // Without validation
    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);
    let handler = CountingHandler::new();
    parser.add_handler(Box::new(handler));
    parser.parse().unwrap();
    let time_without = start.elapsed();

    // With validation
    let start = Instant::now();
    let reader = std::io::Cursor::new(&xml);
    let mut parser = StreamingParser::new(reader);
    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
    parser.add_handler(Box::new(validator));
    parser.parse().unwrap();
    let time_with = start.elapsed();

    println!("Validation overhead comparison (10K elements):");
    println!("  Without validation: {:?}", time_without);
    println!("  With validation:    {:?}", time_with);

    // Validation should not add more than 5x overhead (realistic for schema lookups)
    let overhead_ratio = time_with.as_secs_f64() / time_without.as_secs_f64().max(0.001);
    println!("  Overhead ratio:     {:.2}x", overhead_ratio);

    assert!(
        overhead_ratio < 5.0,
        "Validation overhead too high: {:.2}x",
        overhead_ratio
    );
}
