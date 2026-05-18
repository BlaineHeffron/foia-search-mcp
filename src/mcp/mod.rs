pub(crate) mod fts_repair;
pub(crate) mod ingestion;
pub mod output;
pub(crate) mod repair;
pub(crate) mod source_params;
pub(crate) mod sources;
pub(crate) mod status;
pub(crate) mod support;
pub mod tools;

#[cfg(test)]
mod fts_repair_tests;
#[cfg(test)]
mod ingestion_tests;
#[cfg(test)]
mod repair_tests;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod support_tests;

pub use tools::FoiaSearchServer;
