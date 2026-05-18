pub mod config;
pub mod errors;
pub mod http;
pub mod index;
pub mod ingest;
pub mod mcp;
pub mod model;
pub mod runtime;
pub mod sources;
pub mod store;

#[cfg(test)]
mod source_registry_tests;

pub use mcp::tools::FoiaSearchServer;
pub use runtime::FoiaSearchRuntime;
