//! Load testing benchmarks for fastxml.
//!
//! Tests different patterns:
//! - Many elements (wide tree)
//! - Deep nesting
//! - Large content per element
//! - CityGML-style documents
//!
//! When built with `--features compare-libxml`, also compares performance with libxml.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::io::{BufRead, Read as _};
use std::time::{Duration, Instant};

use std::sync::Arc;

use fastxml::generator::{GeneratorConfig, ProcessingStats, XmlStreamGenerator};
use fastxml::schema::Validator;
use fastxml::schema::types::{CompiledSchema, ElementDef};
use fastxml::{Parser, QueryExt, XmlDocument};

/// Parses XML bytes into a DOM via the public [`Parser`] front door.
fn parse(input: &[u8]) -> fastxml::error::Result<XmlDocument> {
    Parser::from(input).parse()
}

/// Streams every event with constant memory (the public streaming front door).
fn count_streaming(reader: impl BufRead) {
    Parser::from_reader(reader)
        .for_each_event(|_event| Ok(()))
        .unwrap();
}

/// Streams and validates against `schema` in a single pass.
fn validate_streaming(reader: impl BufRead, schema: &Arc<CompiledSchema>) {
    Validator::from_reader(reader)
        .schema(Arc::clone(schema))
        .run()
        .unwrap();
}

// =============================================================================
// libxml comparison (when feature enabled)
// =============================================================================

#[cfg(feature = "compare-libxml")]
mod libxml_bench {
    use libxml::parser::Parser;

    pub fn parse_with_libxml(xml: &[u8]) -> usize {
        let parser = Parser::default();
        let doc = parser.parse_string(xml).unwrap();
        doc.get_root_element().map(|_| 1).unwrap_or(0)
    }

    pub fn xpath_with_libxml(xml: &[u8], xpath: &str) -> usize {
        let parser = Parser::default();
        let doc = parser.parse_string(xml).unwrap();
        let ctx = libxml::xpath::Context::new(&doc).unwrap();
        let result = ctx.evaluate(xpath).unwrap();
        result.get_nodes_as_vec().len()
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
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "CityModel"),
            ElementDef::new("CityModel"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "Building"),
            ElementDef::new("Building"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "measuredHeight"),
            ElementDef::new("measuredHeight"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "name"),
            ElementDef::new("name"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "lod0FootPrint"),
            ElementDef::new("lod0FootPrint"),
        );
    } else {
        // Simple schema for many-elements pattern
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "root"),
            ElementDef::new("root"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "element"),
            ElementDef::new("element"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "item"),
            ElementDef::new("item"),
        );
        schema.elements_ns.insert(
            fastxml::schema::types::NsName::new("", "data"),
            ElementDef::new("data"),
        );
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

        group.bench_with_input(BenchmarkId::new("fastxml_dom", count), &xml, |b, xml| {
            b.iter(|| {
                let doc = parse(black_box(xml)).unwrap();
                black_box(doc.node_count())
            })
        });

        group.bench_with_input(
            BenchmarkId::new("fastxml_streaming", count),
            &xml,
            |b, xml| {
                b.iter(|| {
                    let reader = std::io::Cursor::new(black_box(xml));
                    count_streaming(reader)
                })
            },
        );

        #[cfg(feature = "compare-libxml")]
        group.bench_with_input(BenchmarkId::new("libxml_dom", count), &xml, |b, xml| {
            b.iter(|| black_box(libxml_bench::parse_with_libxml(black_box(xml))))
        });
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

        group.bench_with_input(BenchmarkId::new("dom_parse", depth), &xml, |b, xml| {
            b.iter(|| {
                let doc = parse(black_box(xml)).unwrap();
                black_box(doc.node_count())
            })
        });

        group.bench_with_input(BenchmarkId::new("streaming", depth), &xml, |b, xml| {
            b.iter(|| {
                let reader = std::io::Cursor::new(black_box(xml));
                count_streaming(reader)
            })
        });
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
                    count_streaming(reader)
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
                    let result = doc.query("//bldg:Building").unwrap();
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
                    count_streaming(reader)
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

    group.bench_function("fastxml_descendant_all", |b| {
        b.iter(|| {
            let result = black_box(&doc).query("//*").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("fastxml_by_name", |b| {
        b.iter(|| {
            let result = black_box(&doc).query("//element").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("fastxml_with_predicate", |b| {
        b.iter(|| {
            let result = black_box(&doc).query("//*[name()='item']").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    group.bench_function("fastxml_direct_path", |b| {
        b.iter(|| {
            let result = black_box(&doc).query("/root/*").unwrap();
            black_box(result.into_nodes().len())
        })
    });

    #[cfg(feature = "compare-libxml")]
    {
        group.bench_function("libxml_descendant_all", |b| {
            b.iter(|| black_box(libxml_bench::xpath_with_libxml(black_box(&xml), "//*")))
        });

        group.bench_function("libxml_by_name", |b| {
            b.iter(|| {
                black_box(libxml_bench::xpath_with_libxml(
                    black_box(&xml),
                    "//element",
                ))
            })
        });

        group.bench_function("libxml_direct_path", |b| {
            b.iter(|| black_box(libxml_bench::xpath_with_libxml(black_box(&xml), "/root/*")))
        });
    }

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
                    count_streaming(reader)
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
                    validate_streaming(reader, &schema)
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
                    count_streaming(reader)
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
                    validate_streaming(reader, &schema)
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
                    validate_streaming(reader, &schema)
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
