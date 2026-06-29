//! Shared core types for the Rift workspace.
//!
//! Pure, serde-friendly data types with no behaviour, so they can be depended on by
//! `rift-http-proxy`, `rift-lint`, and `rift-tui` without circular dependencies.

pub mod predicate;

pub use predicate::{Predicate, PredicateOperation, PredicateParameters, PredicateSelector};
