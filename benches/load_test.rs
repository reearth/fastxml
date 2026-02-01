//! Load testing benchmarks for fastxml.
//!
//! Tests different patterns:
//! - Many elements (wide tree)
//! - Deep nesting
//! - Large content per element
//! - CityGML-style documents

use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput,
};
use std::io::{BufRead, Read as _};
use std::time::{Duration, Instant};

use std::sync::Arc;

use fastxml::generator::{GeneratorConfig, ProcessingStats, XmlStreamGenerator};
use fastxml::event::{StreamingParser, XmlEvent, XmlEventHandler};
use fastxml::error::Result;
use fastxml::schema::types::{CompiledSchema, ElementDef};
use fastxml::schema::validator::StreamingSchemaValidator;
use fastxml::{parse, evaluate};

// =============================================================================
// Streaming SAX Handler for Counting
// =============================================================================

/// A handler that counts elements and tracks depth.
struct CountingHandler {
    element_count: usize,
    max_depth: usize,
    current_depth: usize,
    text_bytes: usize,
}

impl CountingHandler {
    fn new() -> Self {
        Self {
            element_count: 0,
            max_depth: 0,
            current_depth: 0,
            text_bytes: 0,
        }
    }
}

impl XmlEventHandler for CountingHandler {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement { .. } => {
                self.element_count += 1;
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

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate XML to a byte vector (for smaller tests).
fn generate_xml_bytes(config: GeneratorConfig) -> Vec<u8> {
    let mut xml_gen = XmlStreamGenerator::new(config);
    let mut output = Vec::new();
    xml_gen.read_to_end(&mut output).unwrap();
    output
}

/// Process XML using streaming parser.
#[allow(dead_code)]
fn process_streaming(reader: impl BufRead) -> ProcessingStats {
    let start = Instant::now();
    let mut parser = StreamingParser::new(reader);
    let handler = CountingHandler::new();
    parser.add_handler(Box::new(handler));

    // We can't easily get the handler back, so we'll count differently
    let _ = parser.parse();

    ProcessingStats {
        bytes_processed: 0, // Not tracked in this version
        element_count: 0,
        max_depth: 0,
        peak_memory: None,
        time_ms: start.elapsed().as_millis(),
    }
}

/// Parse XML into DOM.
#[allow(dead_code)]
fn process_dom(xml: &[u8]) -> ProcessingStats {
    let start = Instant::now();
    let doc = parse(xml).unwrap();
    let element_count = doc.node_count();

    ProcessingStats {
        bytes_processed: xml.len(),
        element_count,
        max_depth: 0,
        peak_memory: None,
        time_ms: start.elapsed().as_millis(),
    }
}

// =============================================================================
// Schema Helpers
// =============================================================================

/// Creates a schema that matches the generated XML patterns.
fn create_test_schema(with_namespaces: bool) -> Arc<CompiledSchema> {
    let mut schema = CompiledSchema::new();

    if with_namespaces {
        // CityGML-style schema
        schema.target_namespace = Some("http://www.opengis.net/citygml/2.0".to_string());
        schema.elements.insert("core:CityModel".to_string(), ElementDef::new("CityModel"));
        schema.elements.insert("bldg:Building".to_string(), ElementDef::new("Building"));
        schema.elements.insert("bldg:measuredHeight".to_string(), ElementDef::new("measuredHeight"));
        schema.elements.insert("gml:name".to_string(), ElementDef::new("name"));
        schema.elements.insert("bldg:lod0FootPrint".to_string(), ElementDef::new("lod0FootPrint"));
    } else {
        // Simple schema for many-elements pattern
        schema.elements.insert("root".to_string(), ElementDef::new("root"));
        schema.elements.insert("element".to_string(), ElementDef::new("element"));
        schema.elements.insert("item".to_string(), ElementDef::new("item"));
        schema.elements.insert("data".to_string(), ElementDef::new("data"));
    }

    Arc::new(schema)
}

// =============================================================================
// Benchmark: Many Elements
// =============================================================================

fn bench_many_elements(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_elements");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for count in [1_000, 10_000, 100_000].iter() {
        let config = GeneratorConfig::many_elements(*count);
        let xml = generate_xml_bytes(config.clone());
        let size = xml.len();

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("dom_parse", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    black_box(doc.node_count())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Deep Nesting
// =============================================================================

fn bench_deep_nesting(c: &mut Criterion) {
    let mut group = c.benchmark_group("deep_nesting");
    group.sample_size(10);

    for depth in [10, 50, 100, 500].iter() {
        let config = GeneratorConfig::deep_nesting(*depth);
        let xml = generate_xml_bytes(config.clone());
        let size = xml.len();

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("dom_parse", depth),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    black_box(doc.node_count())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming", depth),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Large Content
// =============================================================================

fn bench_large_content(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_content");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    // Content sizes in bytes
    for size_kb in [1, 10, 100, 1000].iter() {
        let content_size = size_kb * 1024;
        let config = GeneratorConfig::large_content(content_size);
        let xml = generate_xml_bytes(config.clone());
        let total_size = xml.len();

        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_with_input(
            BenchmarkId::new("dom_parse", format!("{}KB", size_kb)),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    black_box(doc.node_count())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming", format!("{}KB", size_kb)),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: CityGML Style
// =============================================================================

fn bench_citygml_style(c: &mut Criterion) {
    let mut group = c.benchmark_group("citygml_style");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    for building_count in [100, 500, 1000].iter() {
        let config = GeneratorConfig::citygml_style(*building_count);
        let xml = generate_xml_bytes(config.clone());
        let size = xml.len();

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("dom_parse", building_count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    black_box(doc.node_count())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("xpath_all_buildings", building_count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    let result = evaluate(&doc, "//bldg:Building").unwrap();
                    black_box(result.into_nodes().len())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("streaming", building_count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: XPath Evaluation
// =============================================================================

fn bench_xpath_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("xpath_evaluation");
    group.sample_size(20);

    // Generate test document
    let config = GeneratorConfig::many_elements(10_000);
    let xml = generate_xml_bytes(config);
    let doc = parse(&xml).unwrap();

    group.bench_function("descendant_all", |b| {
        b.iter(|| {
            let result = evaluate(black_box(&doc), "//*").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("by_name", |b| {
        b.iter(|| {
            let result = evaluate(black_box(&doc), "//element").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("with_predicate", |b| {
        b.iter(|| {
            let result = evaluate(black_box(&doc), "//*[name()='item']").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("direct_path", |b| {
        b.iter(|| {
            let result = evaluate(black_box(&doc), "/root/*").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.finish();
}

// =============================================================================
// Benchmark: Memory Efficiency (Streaming vs DOM)
// =============================================================================

fn bench_streaming_memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_vs_dom");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));

    // Test with increasingly large documents
    for element_count in [10_000, 50_000, 100_000].iter() {
        let config = GeneratorConfig::many_elements(*element_count);
        let estimated_size = config.estimated_size();

        group.throughput(Throughput::Bytes(estimated_size as u64));

        // Streaming from generator (no intermediate buffer)
        group.bench_with_input(
            BenchmarkId::new("streaming_from_generator", element_count),
            element_count,
            |b, &count| {
                b.iter(|| {
                    let xml_gen = XmlStreamGenerator::many_elements(count);
                    let reader = std::io::BufReader::new(xml_gen);
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );

        // DOM parse (requires full document in memory)
        let xml = generate_xml_bytes(config.clone());
        group.bench_with_input(
            BenchmarkId::new("dom_from_bytes", element_count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let doc = parse(black_box(xml)).unwrap();
                    black_box(doc.node_count())
                })
            },
        );
    }

    group.finish();
}

// =============================================================================
// Benchmark: Schema Validation
// =============================================================================

fn bench_schema_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_validation");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    // Test schema validation with different element counts
    for count in [1_000, 10_000, 50_000].iter() {
        let config = GeneratorConfig::many_elements(*count);
        let xml = generate_xml_bytes(config.clone());
        let size = xml.len();
        let schema = create_test_schema(false);

        group.throughput(Throughput::Bytes(size as u64));

        // Streaming with schema validation
        group.bench_with_input(
            BenchmarkId::new("streaming_with_validation", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
                    parser.add_handler(Box::new(validator));
                    parser.parse().unwrap()
                })
            },
        );

        // Streaming without validation (for comparison)
        group.bench_with_input(
            BenchmarkId::new("streaming_without_validation", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    parser.add_handler(Box::new(handler));
                    parser.parse().unwrap()
                })
            },
        );

        // Streaming with both counting and validation
        group.bench_with_input(
            BenchmarkId::new("streaming_count_and_validate", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let handler = CountingHandler::new();
                    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
                    parser.add_handler(Box::new(handler));
                    parser.add_handler(Box::new(validator));
                    parser.parse().unwrap()
                })
            },
        );
    }

    // CityGML-style with namespace validation
    for building_count in [100, 500].iter() {
        let config = GeneratorConfig::citygml_style(*building_count);
        let xml = generate_xml_bytes(config.clone());
        let size = xml.len();
        let schema = create_test_schema(true);

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::new("citygml_with_validation", building_count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    let mut parser = StreamingParser::new(reader);
                    let validator = StreamingSchemaValidator::new(Arc::clone(&schema));
                    parser.add_handler(Box::new(validator));
                    parser.parse().unwrap()
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_many_elements,
    bench_deep_nesting,
    bench_large_content,
    bench_citygml_style,
    bench_xpath_evaluation,
    bench_streaming_memory,
    bench_schema_validation,
);

criterion_main!(benches);
