//! Profiling utilities for memory and performance measurement.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::document::XmlDocument;
use crate::error::Result;

/// Profile result containing timing and memory statistics.
#[derive(Debug, Clone)]
pub struct ProfileResult {
    /// Time taken to parse the XML.
    pub parse_time: Duration,
    /// Peak memory usage (if available).
    pub memory_peak: Option<usize>,
    /// Current memory usage after parsing.
    pub memory_current: Option<usize>,
    /// Number of nodes in the document.
    pub node_count: usize,
    /// File size in bytes.
    pub file_size: usize,
    /// Time for XPath evaluation (if measured).
    pub xpath_eval_time: Option<Duration>,
    /// Additional metrics.
    pub metrics: ProfileMetrics,
}

impl ProfileResult {
    /// Returns nodes per second parsing rate.
    pub fn nodes_per_second(&self) -> f64 {
        let secs = self.parse_time.as_secs_f64();
        if secs > 0.0 {
            self.node_count as f64 / secs
        } else {
            0.0
        }
    }

    /// Returns bytes per second parsing rate.
    pub fn bytes_per_second(&self) -> f64 {
        let secs = self.parse_time.as_secs_f64();
        if secs > 0.0 {
            self.file_size as f64 / secs
        } else {
            0.0
        }
    }

    /// Returns memory per node ratio.
    pub fn memory_per_node(&self) -> Option<f64> {
        self.memory_current.map(|mem| {
            if self.node_count > 0 {
                mem as f64 / self.node_count as f64
            } else {
                0.0
            }
        })
    }
}

impl std::fmt::Display for ProfileResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Profile Results:")?;
        writeln!(f, "  File size:      {} bytes", self.file_size)?;
        writeln!(f, "  Parse time:     {:?}", self.parse_time)?;
        writeln!(f, "  Node count:     {}", self.node_count)?;
        writeln!(
            f,
            "  Parse rate:     {:.0} nodes/sec",
            self.nodes_per_second()
        )?;
        writeln!(
            f,
            "  Throughput:     {:.2} MB/sec",
            self.bytes_per_second() / 1_000_000.0
        )?;

        if let Some(mem) = self.memory_current {
            writeln!(
                f,
                "  Memory used:    {} bytes ({:.2} MB)",
                mem,
                mem as f64 / 1_000_000.0
            )?;
        }

        if let Some(mem_per_node) = self.memory_per_node() {
            writeln!(f, "  Memory/node:    {:.1} bytes", mem_per_node)?;
        }

        if let Some(xpath_time) = self.xpath_eval_time {
            writeln!(f, "  XPath eval:     {:?}", xpath_time)?;
        }

        Ok(())
    }
}

/// Additional profiling metrics.
#[derive(Debug, Clone, Default)]
pub struct ProfileMetrics {
    /// Number of element nodes.
    pub element_count: usize,
    /// Number of text nodes.
    pub text_count: usize,
    /// Number of attributes.
    pub attribute_count: usize,
    /// Maximum tree depth.
    pub max_depth: usize,
    /// Number of distinct element names.
    pub distinct_elements: usize,
    /// Number of namespace declarations.
    pub namespace_count: usize,
}

/// Gets current memory usage.
#[cfg(feature = "profile")]
pub fn get_memory_usage() -> Option<usize> {
    memory_stats::memory_stats().map(|stats| stats.physical_mem)
}

/// Gets current memory usage (stub when profile feature is disabled).
#[cfg(not(feature = "profile"))]
pub fn get_memory_usage() -> Option<usize> {
    None
}

/// Profiles parsing a file.
pub fn profile_file(path: &Path) -> Result<ProfileResult> {
    use crate::{parse, xpath};

    let file_size = std::fs::metadata(path)?.len() as usize;
    let content = std::fs::read(path)?;

    let memory_before = get_memory_usage();
    let start = Instant::now();

    let doc = parse(&content)?;

    let parse_time = start.elapsed();
    let memory_after = get_memory_usage();

    let memory_current = match (memory_before, memory_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        (None, Some(after)) => Some(after),
        _ => None,
    };

    // Collect metrics
    let metrics = collect_metrics(&doc);
    let node_count = doc.node_count();

    // Measure XPath evaluation
    let xpath_start = Instant::now();
    let _ = xpath::evaluate(&doc, "//*");
    let xpath_eval_time = Some(xpath_start.elapsed());

    Ok(ProfileResult {
        parse_time,
        memory_peak: None, // Would need more sophisticated tracking
        memory_current,
        node_count,
        file_size,
        xpath_eval_time,
        metrics,
    })
}

/// Profiles parsing XML content.
pub fn profile_content(content: &[u8]) -> Result<ProfileResult> {
    use crate::{parse, xpath};

    let file_size = content.len();

    let memory_before = get_memory_usage();
    let start = Instant::now();

    let doc = parse(content)?;

    let parse_time = start.elapsed();
    let memory_after = get_memory_usage();

    let memory_current = match (memory_before, memory_after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        (None, Some(after)) => Some(after),
        _ => None,
    };

    let metrics = collect_metrics(&doc);
    let node_count = doc.node_count();

    // Measure XPath evaluation
    let xpath_start = Instant::now();
    let _ = xpath::evaluate(&doc, "//*");
    let xpath_eval_time = Some(xpath_start.elapsed());

    Ok(ProfileResult {
        parse_time,
        memory_peak: None,
        memory_current,
        node_count,
        file_size,
        xpath_eval_time,
        metrics,
    })
}

/// Collects detailed metrics from a document.
fn collect_metrics(doc: &XmlDocument) -> ProfileMetrics {
    use crate::node::NodeType;
    use std::collections::HashSet;

    let mut metrics = ProfileMetrics::default();
    let mut element_names = HashSet::new();

    fn visit(
        node: &crate::node::XmlNode,
        depth: usize,
        metrics: &mut ProfileMetrics,
        element_names: &mut HashSet<String>,
    ) {
        match node.get_type() {
            NodeType::Element => {
                metrics.element_count += 1;
                element_names.insert(node.qname());
                metrics.attribute_count += node.get_attributes().len();
                metrics.namespace_count += node.get_namespace_declarations().len();
            }
            NodeType::Text => {
                metrics.text_count += 1;
            }
            _ => {}
        }

        metrics.max_depth = metrics.max_depth.max(depth);

        for child in node.get_child_nodes() {
            visit(&child, depth + 1, metrics, element_names);
        }
    }

    if let Ok(root) = doc.get_root_element() {
        visit(&root, 1, &mut metrics, &mut element_names);
    }

    metrics.distinct_elements = element_names.len();
    metrics
}

/// Benchmark helper for comparing performance.
pub struct Benchmark {
    iterations: usize,
    results: Vec<Duration>,
}

impl Benchmark {
    /// Creates a new benchmark.
    pub fn new(iterations: usize) -> Self {
        Self {
            iterations,
            results: Vec::with_capacity(iterations),
        }
    }

    /// Runs a benchmark function.
    pub fn run<F>(&mut self, mut f: F) -> &Self
    where
        F: FnMut(),
    {
        self.results.clear();
        for _ in 0..self.iterations {
            let start = Instant::now();
            f();
            self.results.push(start.elapsed());
        }
        self
    }

    /// Returns the minimum time.
    pub fn min(&self) -> Duration {
        self.results.iter().cloned().min().unwrap_or_default()
    }

    /// Returns the maximum time.
    pub fn max(&self) -> Duration {
        self.results.iter().cloned().max().unwrap_or_default()
    }

    /// Returns the average time.
    pub fn avg(&self) -> Duration {
        if self.results.is_empty() {
            Duration::default()
        } else {
            let sum: Duration = self.results.iter().sum();
            sum / self.results.len() as u32
        }
    }

    /// Returns the median time.
    pub fn median(&self) -> Duration {
        if self.results.is_empty() {
            return Duration::default();
        }
        let mut sorted = self.results.clone();
        sorted.sort();
        sorted[sorted.len() / 2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_content() {
        let xml = r#"<root><child attr="value">text</child></root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();

        assert!(result.parse_time > Duration::ZERO);
        assert_eq!(result.file_size, xml.len());
        assert!(result.node_count > 0);
        assert_eq!(result.metrics.element_count, 2); // root + child
        assert_eq!(result.metrics.attribute_count, 1);
    }

    #[test]
    fn test_benchmark() {
        let mut bench = Benchmark::new(3);
        bench.run(|| {
            std::thread::sleep(std::time::Duration::from_millis(1));
        });

        assert!(bench.min() >= Duration::from_millis(1));
        assert!(bench.avg() >= Duration::from_millis(1));
    }

    // ProfileResult tests
    #[test]
    fn test_profile_result_nodes_per_second() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: None,
            node_count: 1000,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.nodes_per_second(), 1000.0);
    }

    #[test]
    fn test_profile_result_nodes_per_second_zero_time() {
        let result = ProfileResult {
            parse_time: Duration::ZERO,
            memory_peak: None,
            memory_current: None,
            node_count: 1000,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.nodes_per_second(), 0.0);
    }

    #[test]
    fn test_profile_result_bytes_per_second() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: None,
            node_count: 1000,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.bytes_per_second(), 5000.0);
    }

    #[test]
    fn test_profile_result_bytes_per_second_zero_time() {
        let result = ProfileResult {
            parse_time: Duration::ZERO,
            memory_peak: None,
            memory_current: None,
            node_count: 1000,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.bytes_per_second(), 0.0);
    }

    #[test]
    fn test_profile_result_memory_per_node() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: Some(10000),
            node_count: 100,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.memory_per_node(), Some(100.0));
    }

    #[test]
    fn test_profile_result_memory_per_node_none() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: None,
            node_count: 100,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.memory_per_node(), None);
    }

    #[test]
    fn test_profile_result_memory_per_node_zero_nodes() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: Some(10000),
            node_count: 0,
            file_size: 5000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        assert_eq!(result.memory_per_node(), Some(0.0));
    }

    #[test]
    fn test_profile_result_display() {
        let result = ProfileResult {
            parse_time: Duration::from_millis(100),
            memory_peak: None,
            memory_current: Some(1000000),
            node_count: 500,
            file_size: 50000,
            xpath_eval_time: Some(Duration::from_millis(10)),
            metrics: ProfileMetrics::default(),
        };
        let display = format!("{}", result);
        assert!(display.contains("Profile Results:"));
        assert!(display.contains("File size:"));
        assert!(display.contains("Parse time:"));
        assert!(display.contains("Node count:"));
        assert!(display.contains("Memory used:"));
        assert!(display.contains("Memory/node:"));
        assert!(display.contains("XPath eval:"));
    }

    #[test]
    fn test_profile_result_display_no_memory() {
        let result = ProfileResult {
            parse_time: Duration::from_millis(100),
            memory_peak: None,
            memory_current: None,
            node_count: 500,
            file_size: 50000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        let display = format!("{}", result);
        assert!(display.contains("Profile Results:"));
        assert!(!display.contains("Memory used:"));
        assert!(!display.contains("XPath eval:"));
    }

    #[test]
    fn test_profile_result_clone() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: Some(100),
            memory_current: Some(200),
            node_count: 50,
            file_size: 1000,
            xpath_eval_time: Some(Duration::from_millis(5)),
            metrics: ProfileMetrics::default(),
        };
        let cloned = result.clone();
        assert_eq!(cloned.node_count, result.node_count);
        assert_eq!(cloned.file_size, result.file_size);
    }

    #[test]
    fn test_profile_result_debug() {
        let result = ProfileResult {
            parse_time: Duration::from_secs(1),
            memory_peak: None,
            memory_current: None,
            node_count: 50,
            file_size: 1000,
            xpath_eval_time: None,
            metrics: ProfileMetrics::default(),
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("ProfileResult"));
    }

    // ProfileMetrics tests
    #[test]
    fn test_profile_metrics_default() {
        let metrics = ProfileMetrics::default();
        assert_eq!(metrics.element_count, 0);
        assert_eq!(metrics.text_count, 0);
        assert_eq!(metrics.attribute_count, 0);
        assert_eq!(metrics.max_depth, 0);
        assert_eq!(metrics.distinct_elements, 0);
        assert_eq!(metrics.namespace_count, 0);
    }

    #[test]
    fn test_profile_metrics_clone() {
        let metrics = ProfileMetrics {
            element_count: 10,
            text_count: 5,
            attribute_count: 3,
            max_depth: 4,
            distinct_elements: 2,
            namespace_count: 1,
        };
        let cloned = metrics.clone();
        assert_eq!(cloned.element_count, 10);
        assert_eq!(cloned.text_count, 5);
    }

    #[test]
    fn test_profile_metrics_debug() {
        let metrics = ProfileMetrics::default();
        let debug = format!("{:?}", metrics);
        assert!(debug.contains("ProfileMetrics"));
    }

    // Benchmark tests
    #[test]
    fn test_benchmark_new() {
        let bench = Benchmark::new(5);
        assert_eq!(bench.iterations, 5);
        assert!(bench.results.is_empty());
    }

    #[test]
    fn test_benchmark_max() {
        let mut bench = Benchmark::new(3);
        bench.run(|| {
            std::thread::sleep(std::time::Duration::from_millis(1));
        });
        assert!(bench.max() >= Duration::from_millis(1));
    }

    #[test]
    fn test_benchmark_median() {
        let mut bench = Benchmark::new(3);
        bench.run(|| {
            std::thread::sleep(std::time::Duration::from_millis(1));
        });
        assert!(bench.median() >= Duration::from_millis(1));
    }

    #[test]
    fn test_benchmark_empty_results() {
        let bench = Benchmark::new(3);
        assert_eq!(bench.min(), Duration::default());
        assert_eq!(bench.max(), Duration::default());
        assert_eq!(bench.avg(), Duration::default());
        assert_eq!(bench.median(), Duration::default());
    }

    #[test]
    fn test_benchmark_run_clears_results() {
        let mut bench = Benchmark::new(2);
        bench.run(|| {});
        assert_eq!(bench.results.len(), 2);
        bench.run(|| {});
        assert_eq!(bench.results.len(), 2);
    }

    // get_memory_usage test
    #[test]
    fn test_get_memory_usage() {
        // Just verify it doesn't panic
        let _ = get_memory_usage();
    }

    // profile_content additional tests
    #[test]
    fn test_profile_content_with_namespaces() {
        let xml = r#"<root xmlns:ns="http://example.com"><ns:child/></root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();
        assert!(result.metrics.namespace_count >= 1);
    }

    #[test]
    fn test_profile_content_nested() {
        let xml = r#"<root><a><b><c><d/></c></b></a></root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();
        assert!(result.metrics.max_depth >= 4);
        assert_eq!(result.metrics.element_count, 5);
    }

    #[test]
    fn test_profile_content_text_nodes() {
        let xml = r#"<root>text1<child>text2</child>text3</root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();
        assert!(result.metrics.text_count >= 2);
    }

    #[test]
    fn test_profile_content_distinct_elements() {
        let xml = r#"<root><a/><b/><a/><c/></root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();
        // root, a, b, c = 4 distinct elements
        assert_eq!(result.metrics.distinct_elements, 4);
    }

    #[test]
    fn test_profile_content_xpath_eval_time() {
        let xml = r#"<root><child/></root>"#;
        let result = profile_content(xml.as_bytes()).unwrap();
        assert!(result.xpath_eval_time.is_some());
    }
}
