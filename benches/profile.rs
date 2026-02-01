//! Benchmarks for fastxml.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use fastxml::{parse, xpath};

fn create_test_xml(depth: usize, breadth: usize) -> String {
    fn build(depth: usize, breadth: usize, current: usize) -> String {
        if current >= depth {
            return "<leaf>text content</leaf>".to_string();
        }
        let children: String = (0..breadth)
            .map(|i| {
                format!(
                    "<child{}>{}  </child{}>",
                    i,
                    build(depth, breadth, current + 1),
                    i
                )
            })
            .collect();
        format!("<node depth=\"{}\">{}</node>", current, children)
    }
    format!(
        r#"<?xml version="1.0"?><root>{}</root>"#,
        build(depth, breadth, 0)
    )
}

fn bench_parsing(c: &mut Criterion) {
    let small_xml = create_test_xml(3, 3);
    let medium_xml = create_test_xml(4, 5);
    let large_xml = create_test_xml(5, 6);

    let mut group = c.benchmark_group("parsing");

    group.throughput(Throughput::Bytes(small_xml.len() as u64));
    group.bench_function("small", |b| {
        b.iter(|| parse(black_box(&small_xml)).unwrap())
    });

    group.throughput(Throughput::Bytes(medium_xml.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| parse(black_box(&medium_xml)).unwrap())
    });

    group.throughput(Throughput::Bytes(large_xml.len() as u64));
    group.bench_function("large", |b| {
        b.iter(|| parse(black_box(&large_xml)).unwrap())
    });

    group.finish();
}

fn bench_xpath(c: &mut Criterion) {
    let xml = create_test_xml(4, 5);
    let doc = parse(&xml).unwrap();

    let mut group = c.benchmark_group("xpath");

    group.bench_function("simple_path", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "/root/node").unwrap())
    });

    group.bench_function("descendant", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "//leaf").unwrap())
    });

    group.bench_function("predicate_name", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "//*[name()='leaf']").unwrap())
    });

    group.bench_function("wildcard", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "//*").unwrap())
    });

    group.finish();
}

fn bench_namespaced(c: &mut Criterion) {
    let xml = r#"<?xml version="1.0"?>
    <gml:FeatureCollection xmlns:gml="http://www.opengis.net/gml"
                           xmlns:bldg="http://www.opengis.net/citygml/building/2.0">
        <gml:featureMember>
            <bldg:Building gml:id="bldg1">
                <bldg:measuredHeight>10.5</bldg:measuredHeight>
            </bldg:Building>
        </gml:featureMember>
        <gml:featureMember>
            <bldg:Building gml:id="bldg2">
                <bldg:measuredHeight>15.2</bldg:measuredHeight>
            </bldg:Building>
        </gml:featureMember>
    </gml:FeatureCollection>"#;

    let doc = parse(xml).unwrap();

    let mut group = c.benchmark_group("namespaced");

    group.bench_function("parse", |b| b.iter(|| parse(black_box(xml)).unwrap()));

    group.bench_function("xpath_namespaced", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "//bldg:Building").unwrap())
    });

    group.bench_function("xpath_text", |b| {
        b.iter(|| xpath::evaluate(black_box(&doc), "//bldg:measuredHeight/text()").unwrap())
    });

    group.finish();
}

criterion_group!(benches, bench_parsing, bench_xpath, bench_namespaced);
criterion_main!(benches);
