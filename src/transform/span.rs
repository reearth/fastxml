//! Byte span tracking for zero-copy output.

/// A span of bytes in the source XML string.
///
/// Used to track positions for zero-copy output of unchanged regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ByteSpan {
    /// Start position (inclusive)
    pub start: usize,
    /// End position (exclusive)
    pub end: usize,
}

impl ByteSpan {
    /// Creates a new byte span.
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "span start must be <= end");
        Self { start, end }
    }

    /// Returns the length of this span.
    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if this span is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Extracts the slice from the input string.
    ///
    /// # Panics
    ///
    /// Panics if the span is out of bounds for the input.
    #[inline]
    pub fn slice<'a>(&self, input: &'a str) -> &'a str {
        &input[self.start..self.end]
    }

    /// Extracts the slice from the input bytes.
    ///
    /// # Panics
    ///
    /// Panics if the span is out of bounds for the input.
    #[inline]
    pub fn slice_bytes<'a>(&self, input: &'a [u8]) -> &'a [u8] {
        &input[self.start..self.end]
    }

    /// Returns true if this span contains the given position.
    #[inline]
    pub fn contains(&self, pos: usize) -> bool {
        pos >= self.start && pos < self.end
    }

    /// Returns true if this span overlaps with another span.
    #[inline]
    pub fn overlaps(&self, other: &ByteSpan) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Extends this span to include another span.
    #[inline]
    pub fn extend(&mut self, other: &ByteSpan) {
        self.start = self.start.min(other.start);
        self.end = self.end.max(other.end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_span_basics() {
        let span = ByteSpan::new(5, 10);
        assert_eq!(span.len(), 5);
        assert!(!span.is_empty());

        let empty = ByteSpan::new(5, 5);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_slice() {
        let input = "Hello, World!";
        let span = ByteSpan::new(7, 12);
        assert_eq!(span.slice(input), "World");
    }

    #[test]
    fn test_contains() {
        let span = ByteSpan::new(5, 10);
        assert!(!span.contains(4));
        assert!(span.contains(5));
        assert!(span.contains(9));
        assert!(!span.contains(10));
    }

    #[test]
    fn test_overlaps() {
        let span1 = ByteSpan::new(5, 10);
        let span2 = ByteSpan::new(8, 15);
        let span3 = ByteSpan::new(10, 15);

        assert!(span1.overlaps(&span2));
        assert!(!span1.overlaps(&span3));
    }
}
