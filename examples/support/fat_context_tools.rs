//! Fat-payload variant of the context-optimization tool set.
//!
//! Include this file only from examples that register padded tool results.

#[path = "../../benches/support/benchmark_tools.rs"]
mod benchmark_tools;

use superglue::Client;

pub use benchmark_tools::FAT_RAW_CHARS;

/// Same tools, each result padded with a bulky `raw` dump of `raw_chars`.
pub async fn register_fat_context_optimization_tools(
    client: &Client,
    raw_chars: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    for tool in benchmark_tools::fat_benchmark_tools(raw_chars).await {
        client.register_tool(tool).await?;
    }
    Ok(())
}
