//! XSD Abstract Syntax Tree (AST) types.
//!
//! These types represent the intermediate representation of an XSD schema
//! after parsing but before compilation into a CompiledSchema.

mod attributes;
mod complex;
mod constraints;
mod elements;
mod groups;
mod occurs;
mod particles;
mod qname;
mod schema;
mod simple;

// Re-export all types
pub use attributes::*;
pub use complex::*;
pub use constraints::*;
pub use elements::*;
pub use groups::*;
pub use occurs::*;
pub use particles::*;
pub use qname::*;
pub use schema::*;
pub use simple::*;

#[cfg(test)]
mod tests;
