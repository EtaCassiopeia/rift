// Library exports for benchmarking and testing
// Allow dead_code for library targets - functions are used by the binary but not by tests
#![allow(dead_code)]

// ===== Core Mountebank-compatible modules =====
pub mod admin_api;
pub mod behaviors;
pub mod config;
pub mod imposter;
pub mod predicate;
pub mod proxy;
pub mod recording;

// ===== Rift Extensions (features beyond Mountebank) =====
pub mod extensions;
pub mod response;

// Re-export extension modules at top level for backward compatibility
pub use extensions::fault;
pub use extensions::flow_state;
pub use extensions::matcher;
pub use extensions::routing;
pub use extensions::rule_index;
pub use extensions::stub_analysis;
pub use extensions::template;

// Don't export internal modules
mod backends;
mod scripting;
