//! XML node representation and operations.

pub mod error;
mod mutable;
mod readonly;
mod types;

#[cfg(test)]
mod tests;

// Re-export public types
pub use mutable::XmlNode;
pub use readonly::XmlRoNode;
pub use types::{NodeId, NodeType};

// Re-export internal types for crate use
pub(crate) use types::NodeData;
