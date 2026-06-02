//! Deprecated API for backward compatibility.

use std::collections::HashMap;
use std::io::Write;

use crate::xpath::{Expr, XPathSource};

use super::FallbackMode;
use super::builder::{Handler, HandlerCallback, StreamTransformer};
use super::editable::EditableNode;
use super::error::{TransformError, TransformResult};
use super::functions::stream_transform_impl;

impl<'a> StreamTransformer<'a> {
    /// Sets the XPath expression for matching elements to transform.
    #[deprecated(since = "0.4.0", note = "use .on(xpath, callback).run() instead")]
    pub fn xpath(mut self, xpath: &str) -> Self {
        // Store as a placeholder handler with no-op callback
        // The actual callback will be set by transform()
        self.handlers.push(Handler {
            xpath: XPathSource::String(xpath.to_string()),
            callback: HandlerCallback::Simple(Box::new(|_| {})),
        });
        self
    }

    /// Sets a pre-parsed XPath AST for matching elements to transform.
    #[deprecated(since = "0.4.0", note = "use .on() with string XPath instead")]
    pub fn xpath_ast(mut self, expr: Expr) -> Self {
        self.handlers.push(Handler {
            xpath: XPathSource::Ast(expr),
            callback: HandlerCallback::Simple(Box::new(|_| {})),
        });
        self
    }

    /// Sets the transform function and returns a builder for final operations.
    #[deprecated(since = "0.4.0", note = "use .on(xpath, callback).run() instead")]
    #[allow(clippy::should_implement_trait, deprecated)]
    pub fn transform<F>(mut self, transform_fn: F) -> StreamTransformBuilder<'a, F>
    where
        F: FnMut(&mut EditableNode),
    {
        // Get the xpath from the last handler (set by xpath())
        let xpath_source = if let Some(handler) = self.handlers.pop() {
            handler.xpath
        } else {
            XPathSource::String(String::new())
        };

        StreamTransformBuilder {
            input: self.input,
            xpath_source,
            namespaces: self.namespaces,
            transform_fn,
        }
    }
}

/// A consuming builder that captures the transform function.
#[deprecated(since = "0.4.0", note = "use StreamTransformer::on().run() instead")]
#[doc(hidden)]
pub struct StreamTransformBuilder<'a, F> {
    input: &'a str,
    xpath_source: XPathSource,
    namespaces: HashMap<String, String>,
    transform_fn: F,
}

#[allow(deprecated)]
impl<'a, F> StreamTransformBuilder<'a, F>
where
    F: FnMut(&mut EditableNode),
{
    /// Writes the transformation result to a writer.
    pub fn write_to<W: Write>(self, writer: &mut W) -> TransformResult<usize> {
        if self
            .xpath_source
            .as_string()
            .map(|s| s.is_empty())
            .unwrap_or(true)
            && matches!(self.xpath_source, XPathSource::String(_))
        {
            return Err(TransformError::InvalidXPath(
                "No XPath expression provided".to_string(),
            ));
        }

        // Deprecated API maintains backward compatibility with fallback enabled
        stream_transform_impl(
            self.input,
            &self.xpath_source,
            &self.namespaces,
            FallbackMode::Enabled,
            self.transform_fn,
            writer,
        )
    }

    /// Returns the transformation result as a String.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(self) -> TransformResult<String> {
        let mut output = Vec::new();
        self.write_to(&mut output)?;
        String::from_utf8(output).map_err(|e| TransformError::Utf8(e.utf8_error()))
    }
}
