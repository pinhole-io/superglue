//! Superglue — polyglot LLM orchestration core.
//!
//! The Rust crate owns provider I/O, the tool loop, streaming, and optional servers.
//! Python, JavaScript, and Kotlin bindings are thin wrappers.
//!
//! Start at [`client`] for `Client` and `ClientConversation`.
//! Provider routing lives in [`providers`]. Chat and tools live in [`chat`] and [`tools`].
//! Design notes are in the `_docs/` directory next to this crate.

pub mod agents;
pub mod audio;
pub mod audit;
pub mod batch;
pub mod cancel;
pub mod chat;
pub mod client;
pub mod content_quality;
pub mod context;
pub mod costing;
pub mod embeddings;
pub mod events;
pub mod fallback;
pub mod files;
pub mod guardrails;
pub mod hooks;
pub mod http;
pub mod images;
pub mod judge;
pub mod openai;
pub mod proto;
pub mod providers;
pub mod responses;
pub mod structured;
pub mod systemone;
pub mod telemetry;
pub mod tools;
pub mod usage;

#[cfg(feature = "realtime")]
pub mod realtime;

pub use usage::{
    UsageBreakdown, accumulate_usage, usage_from_breakdown, usage_from_compat, usage_to_json,
};

#[cfg(feature = "mcp")]
pub mod mcp;

pub use client::{CallOptions, Client, ClientBuildError, ClientBuilder, ClientConversation};

#[cfg(feature = "grpc")]
pub mod grpc;

#[cfg(feature = "gateway")]
pub mod gateway;

/// Returns a short version string for the crate.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Core greeting used by the CLI `hello` command.
#[must_use]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }

    #[test]
    fn greet_includes_name() {
        assert_eq!(greet("world"), "Hello, world!");
    }
}
