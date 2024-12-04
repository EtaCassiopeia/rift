// Library exports for benchmarking and testing
// Allow dead_code for library targets - functions are used by the binary but not by tests
#![allow(dead_code)]

pub mod admin_api;
pub mod behaviors;
pub mod config;
pub mod fault;
pub mod flow_state;
pub mod imposter;
pub mod matcher;
pub mod predicate;
pub mod proxy;
pub mod recording;
pub mod routing;
pub mod rule_index;
pub mod stub_analysis;
pub mod template;

// Don't export internal modules
mod backends;
mod metrics;
mod scripting;
