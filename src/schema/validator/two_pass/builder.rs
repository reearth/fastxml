//! Skeleton builder for the first pass of two-pass validation.

use std::sync::Arc;

use crate::error::Result;
use crate::event::{XmlEvent, XmlEventHandler};

use super::skeleton::{DocumentSkeleton, ElementSkeleton};

/// Builder for creating a document skeleton during the first pass.
pub(crate) struct SkeletonBuilder {
    /// The document skeleton being built
    pub(crate) skeleton: DocumentSkeleton,
    /// Stack of node indices representing the current path
    stack: Vec<usize>,
}

impl SkeletonBuilder {
    /// Creates a new skeleton builder.
    pub fn new() -> Self {
        Self {
            skeleton: DocumentSkeleton::new(),
            stack: Vec::with_capacity(64),
        }
    }

    /// Consumes the builder and returns the built skeleton.
    pub fn into_skeleton(self) -> DocumentSkeleton {
        self.skeleton
    }
}

impl XmlEventHandler for SkeletonBuilder {
    fn handle(&mut self, event: &XmlEvent) -> Result<()> {
        match event {
            XmlEvent::StartElement {
                name,
                prefix,
                line,
                column,
                ..
            } => {
                // Create new skeleton node
                let node = ElementSkeleton::new(
                    Arc::clone(name),
                    prefix.as_ref().map(Arc::clone),
                    *line,
                    *column,
                );

                // Add to flat storage
                let node_index = self.skeleton.nodes.len();
                self.skeleton.nodes.push(node);

                // Update parent's child info
                if let Some(&parent_idx) = self.stack.last() {
                    // Increment parent's child count
                    if let Some(parent) = self.skeleton.nodes.get_mut(parent_idx) {
                        parent.increment_child(name.as_ref());
                        parent.children_indices.push(node_index);
                    }
                } else {
                    // This is the root element
                    self.skeleton.root_index = Some(node_index);
                }

                // Push current node onto stack
                self.stack.push(node_index);
            }
            XmlEvent::EndElement { .. } => {
                // Pop the current element from stack
                self.stack.pop();
            }
            XmlEvent::Text(text) | XmlEvent::CData(text) => {
                // Append text to current element
                if let Some(&current_idx) = self.stack.last() {
                    if let Some(current) = self.skeleton.nodes.get_mut(current_idx) {
                        current.text_content.push_str(text);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn as_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}
