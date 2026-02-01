//! XML stream generator for load testing.
//!
//! This module provides generators that produce large XML documents
//! without holding the entire content in memory.

use std::io::{self, BufRead, Read, Write};

/// Configuration for XML generation.
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Total number of elements to generate
    pub element_count: usize,
    /// Maximum nesting depth
    pub max_depth: usize,
    /// Size of text content per element (bytes)
    pub content_size: usize,
    /// Number of attributes per element
    pub attribute_count: usize,
    /// Whether to include namespaces
    pub with_namespaces: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            element_count: 1000,
            max_depth: 10,
            content_size: 100,
            attribute_count: 3,
            with_namespaces: false,
        }
    }
}

impl GeneratorConfig {
    /// Creates a config for testing many elements (wide tree).
    pub fn many_elements(count: usize) -> Self {
        Self {
            element_count: count,
            max_depth: 3,
            content_size: 50,
            attribute_count: 2,
            with_namespaces: false,
        }
    }

    /// Creates a config for testing deep nesting.
    pub fn deep_nesting(depth: usize) -> Self {
        Self {
            element_count: depth * 2,
            max_depth: depth,
            content_size: 20,
            attribute_count: 1,
            with_namespaces: false,
        }
    }

    /// Creates a config for testing large content.
    pub fn large_content(size_bytes: usize) -> Self {
        Self {
            element_count: 10,
            max_depth: 3,
            content_size: size_bytes,
            attribute_count: 1,
            with_namespaces: false,
        }
    }

    /// Creates a config mimicking CityGML structure.
    pub fn citygml_style(building_count: usize) -> Self {
        Self {
            element_count: building_count * 20, // Each building has ~20 elements
            max_depth: 8,
            content_size: 100,
            attribute_count: 3,
            with_namespaces: true,
        }
    }

    /// Estimates the total size in bytes.
    pub fn estimated_size(&self) -> usize {
        let element_overhead = 50; // <element attr="value"></element>
        let attr_overhead = self.attribute_count * 20;
        let per_element = element_overhead + attr_overhead + self.content_size;
        self.element_count * per_element
    }
}

/// A streaming XML generator that implements Read.
///
/// This allows generating gigabytes of XML without holding it all in memory.
pub struct XmlStreamGenerator {
    config: GeneratorConfig,
    state: GeneratorState,
    buffer: Vec<u8>,
    buffer_pos: usize,
    elements_generated: usize,
    current_depth: usize,
    element_stack: Vec<String>,
    finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GeneratorState {
    Header,
    OpenRoot,
    GenerateElements,
    CloseElements,
    CloseRoot,
    Done,
}

impl XmlStreamGenerator {
    /// Creates a new XML stream generator.
    pub fn new(config: GeneratorConfig) -> Self {
        Self {
            config,
            state: GeneratorState::Header,
            buffer: Vec::with_capacity(64 * 1024), // 64KB buffer
            buffer_pos: 0,
            elements_generated: 0,
            current_depth: 0,
            element_stack: Vec::new(),
            finished: false,
        }
    }

    /// Creates a generator for many elements pattern.
    pub fn many_elements(count: usize) -> Self {
        Self::new(GeneratorConfig::many_elements(count))
    }

    /// Creates a generator for deep nesting pattern.
    pub fn deep_nesting(depth: usize) -> Self {
        Self::new(GeneratorConfig::deep_nesting(depth))
    }

    /// Creates a generator for large content pattern.
    pub fn large_content(size_bytes: usize) -> Self {
        Self::new(GeneratorConfig::large_content(size_bytes))
    }

    /// Creates a generator for CityGML-style documents.
    pub fn citygml_style(building_count: usize) -> Self {
        Self::new(GeneratorConfig::citygml_style(building_count))
    }

    fn fill_buffer(&mut self) {
        self.buffer.clear();
        self.buffer_pos = 0;

        match self.state {
            GeneratorState::Header => {
                self.write_header();
                self.state = GeneratorState::OpenRoot;
            }
            GeneratorState::OpenRoot => {
                self.write_root_open();
                self.state = GeneratorState::GenerateElements;
            }
            GeneratorState::GenerateElements => {
                self.generate_elements_batch();
                if self.elements_generated >= self.config.element_count {
                    self.state = GeneratorState::CloseElements;
                }
            }
            GeneratorState::CloseElements => {
                self.close_remaining_elements();
                self.state = GeneratorState::CloseRoot;
            }
            GeneratorState::CloseRoot => {
                self.write_root_close();
                self.state = GeneratorState::Done;
            }
            GeneratorState::Done => {
                self.finished = true;
            }
        }
    }

    fn write_header(&mut self) {
        let _ = writeln!(self.buffer, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    }

    fn write_root_open(&mut self) {
        if self.config.with_namespaces {
            let _ = write!(
                self.buffer,
                r#"<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                 xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                 xmlns:gml="http://www.opengis.net/gml"
                 xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
"#
            );
            self.element_stack.push("core:CityModel".to_string());
        } else {
            let _ = writeln!(self.buffer, "<root>");
            self.element_stack.push("root".to_string());
        }
        self.current_depth = 1;
    }

    fn write_root_close(&mut self) {
        if let Some(tag) = self.element_stack.pop() {
            let _ = writeln!(self.buffer, "</{}>", tag);
        }
    }

    fn generate_elements_batch(&mut self) {
        let batch_size = 100.min(self.config.element_count - self.elements_generated);

        for _ in 0..batch_size {
            if self.elements_generated >= self.config.element_count {
                break;
            }

            self.generate_single_element();
            self.elements_generated += 1;
        }
    }

    fn generate_single_element(&mut self) {
        let indent = "  ".repeat(self.current_depth);

        // Decide whether to go deeper, stay, or go up
        let should_nest =
            self.current_depth < self.config.max_depth && self.elements_generated.is_multiple_of(5); // Nest every 5th element

        let element_name = if self.config.with_namespaces {
            match self.elements_generated % 4 {
                0 => "bldg:Building",
                1 => "bldg:measuredHeight",
                2 => "gml:name",
                _ => "bldg:lod0FootPrint",
            }
        } else {
            match self.elements_generated % 3 {
                0 => "element",
                1 => "item",
                _ => "data",
            }
        };

        // Write opening tag with attributes
        let _ = write!(self.buffer, "{}<{}", indent, element_name);

        // Add attributes
        for i in 0..self.config.attribute_count {
            let _ = write!(
                self.buffer,
                " attr{}=\"value{}\"",
                i, self.elements_generated
            );
        }

        if self.config.with_namespaces && element_name.contains("Building") {
            let _ = write!(
                self.buffer,
                " gml:id=\"bldg_{:08}\"",
                self.elements_generated
            );
        }

        let _ = write!(self.buffer, ">");

        // Add content
        if self.config.content_size > 0 {
            self.write_content();
        }

        if should_nest {
            let _ = writeln!(self.buffer);
            self.element_stack.push(element_name.to_string());
            self.current_depth += 1;
        } else {
            // Close immediately
            let _ = writeln!(self.buffer, "</{}>", element_name);

            // Maybe close a parent
            if self.current_depth > 1
                && self.elements_generated.is_multiple_of(7)
                && let Some(parent) = self.element_stack.pop()
            {
                if parent != "root" && parent != "core:CityModel" {
                    self.current_depth -= 1;
                    let parent_indent = "  ".repeat(self.current_depth);
                    let _ = writeln!(self.buffer, "{}</{}>", parent_indent, parent);
                } else {
                    self.element_stack.push(parent);
                }
            }
        }
    }

    fn write_content(&mut self) {
        // Generate content without allocating a huge string
        let content_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 ";
        let chars: Vec<char> = content_chars.chars().collect();
        let mut remaining = self.config.content_size;

        while remaining > 0 {
            let chunk_size = remaining.min(1024);
            for i in 0..chunk_size {
                let c = chars[(self.elements_generated + i) % chars.len()];
                let _ = write!(self.buffer, "{}", c);
            }
            remaining -= chunk_size;
        }
    }

    fn close_remaining_elements(&mut self) {
        while let Some(tag) = self.element_stack.pop() {
            if tag == "root" || tag == "core:CityModel" {
                self.element_stack.push(tag);
                break;
            }
            self.current_depth -= 1;
            let indent = "  ".repeat(self.current_depth);
            let _ = writeln!(self.buffer, "{}</{}>", indent, tag);
        }
    }
}

impl Read for XmlStreamGenerator {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.finished && self.buffer_pos >= self.buffer.len() {
            return Ok(0);
        }

        // Fill buffer if empty
        if self.buffer_pos >= self.buffer.len() {
            self.fill_buffer();
        }

        // Copy from buffer to output
        let available = self.buffer.len() - self.buffer_pos;
        let to_copy = available.min(buf.len());

        if to_copy > 0 {
            buf[..to_copy]
                .copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + to_copy]);
            self.buffer_pos += to_copy;
        }

        Ok(to_copy)
    }
}

impl BufRead for XmlStreamGenerator {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.buffer_pos >= self.buffer.len() && !self.finished {
            self.fill_buffer();
        }
        Ok(&self.buffer[self.buffer_pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.buffer_pos = (self.buffer_pos + amt).min(self.buffer.len());
    }
}

/// Statistics from parsing/processing.
#[derive(Debug, Clone, Default)]
pub struct ProcessingStats {
    /// Total bytes processed
    pub bytes_processed: usize,
    /// Total elements found
    pub element_count: usize,
    /// Maximum depth encountered
    pub max_depth: usize,
    /// Peak memory usage (if tracked)
    pub peak_memory: Option<usize>,
    /// Processing time in milliseconds
    pub time_ms: u128,
}

impl ProcessingStats {
    /// Throughput in MB/s
    pub fn throughput_mb_per_sec(&self) -> f64 {
        if self.time_ms == 0 {
            return 0.0;
        }
        let mb = self.bytes_processed as f64 / (1024.0 * 1024.0);
        let secs = self.time_ms as f64 / 1000.0;
        mb / secs
    }

    /// Elements per second
    pub fn elements_per_sec(&self) -> f64 {
        if self.time_ms == 0 {
            return 0.0;
        }
        let secs = self.time_ms as f64 / 1000.0;
        self.element_count as f64 / secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_generator_basic() {
        let config = GeneratorConfig {
            element_count: 10,
            max_depth: 3,
            content_size: 20,
            attribute_count: 2,
            with_namespaces: false,
        };

        let mut xml_gen = XmlStreamGenerator::new(config);
        let mut output = String::new();
        xml_gen.read_to_string(&mut output).unwrap();

        assert!(output.starts_with("<?xml"));
        assert!(output.contains("<root>"));
        assert!(output.contains("</root>"));
        assert!(output.contains("<element"));
    }

    #[test]
    fn test_generator_namespaced() {
        let config = GeneratorConfig::citygml_style(2);
        let mut xml_gen = XmlStreamGenerator::new(config);
        let mut output = String::new();
        xml_gen.read_to_string(&mut output).unwrap();

        assert!(output.contains("xmlns:bldg"));
        assert!(output.contains("bldg:Building"));
    }

    #[test]
    fn test_generator_streaming() {
        // Test that we can read in chunks without loading everything
        let config = GeneratorConfig::many_elements(1000);
        let mut xml_gen = XmlStreamGenerator::new(config);

        let mut total_bytes = 0;
        let mut chunk = [0u8; 4096];

        loop {
            let n = xml_gen.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            total_bytes += n;
        }

        assert!(total_bytes > 10000); // Should be substantial
    }

    #[test]
    fn test_estimated_size() {
        let config = GeneratorConfig::many_elements(1000);
        let estimated = config.estimated_size();

        let mut xml_gen = XmlStreamGenerator::new(config);
        let mut output = Vec::new();
        xml_gen.read_to_end(&mut output).unwrap();

        // Should be within 50% of estimate
        let ratio = output.len() as f64 / estimated as f64;
        assert!(ratio > 0.5 && ratio < 2.0, "ratio: {}", ratio);
    }
}
