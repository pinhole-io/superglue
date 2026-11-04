//! Example 39: Resume a crashed tool run from a `JsonlRunStore`.
//!
//! The example runs itself twice:
//!
//! 1. A child process starts a tool run. A hook exits the process before the
//!    second model call. The tool round is already on disk.
//! 2. The parent reads the partial run and finishes it. The tool does not run again.
//!
//! Run with:
//! `OPENAI_API_KEY=... cargo run --example 39_jsonl_resume`
//!
//! The JSONL file holds the prompt and the tool output in plain text.
//! It never holds the API key.

mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use serde_json::json;
use superglue::audit::{AuditStore, JsonlRunStore, resume_context_from_record};
use superglue::chat::ChatOptions;
use superglue::hooks::{
    HookConfig, HookContext, HookError, HookErrorStrategy, HookHandler, HookStage,
};
use superglue::tools::ToolSpec;
use superglue::{CallOptions, Client};
use support::{FnTool, base_url, model, require_api_key};

const REQUEST_ID: &str = "order-42-lookup";
const CRASH_FLAG: &str = "--crash";
const CRASH_EXIT: i32 = 3;

/// Exits the process on the second model call, after the first tool round is logged.
struct CrashBeforeSecondCall {
    calls: AtomicU32,
}

#[async_trait]
impl HookHandler for CrashBeforeSecondCall {
    async fn execute(&self, ctx: HookContext) -> Result<HookContext, HookError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
            println!("  child: simulated crash before the second model call");
            std::process::exit(CRASH_EXIT);
        }
        Ok(ctx)
    }
}

/// Both runs share the request id. Tool calls on chat completions need reasoning off.
fn call_options() -> CallOptions {
    CallOptions {
        request_id: Some(REQUEST_ID.into()),
        reasoning_effort: Some("none".into()),
        ..CallOptions::default()
    }
}

fn store_dir() -> PathBuf {
    std::env::temp_dir().join("superglue-example-39")
}

async fn client(store: Arc<JsonlRunStore>) -> Result<Client, Box<dyn std::error::Error>> {
    let client = Client::builder()
        .api_key(require_api_key())
        .model(model())
        .base_url(base_url())
        .audit(store)
        .build()?;
    client
        .register_tool(
            FnTool::new(
                ToolSpec {
                    name: "lookup_order".into(),
                    description: Some("Return the status of an order by id.".into()),
                    parameters_schema: json!({
                        "type": "object",
                        "properties": { "order_id": { "type": "string" } },
                        "required": ["order_id"]
                    }),
                    static_tool: false,
                },
                |args| {
                    println!("  tool: lookup_order({args})");
                    Ok(json!({ "order_id": args["order_id"], "status": "shipped", "eta_days": 2 }))
                },
            )
            .into_arc(),
        )
        .await?;
    Ok(client)
}

async fn crash_run(store: Arc<JsonlRunStore>) -> Result<(), Box<dyn std::error::Error>> {
    let client = client(store).await?;
    client
        .register_hook(
            HookStage::PreCompletion,
            HookConfig {
                name: "crash".into(),
                error_strategy: HookErrorStrategy::Skip,
                handler: Arc::new(CrashBeforeSecondCall {
                    calls: AtomicU32::new(0),
                }),
            },
        )
        .await;
    let outcome = client
        .complete(
            "Use lookup_order for order 42, then tell me its status in one sentence.",
            call_options(),
        )
        .await?;
    println!(
        "  child: the model answered without a tool call: {}",
        outcome.content.unwrap_or_default()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    require_api_key();
    let dir = store_dir();

    if std::env::args().any(|arg| arg == CRASH_FLAG) {
        return crash_run(Arc::new(JsonlRunStore::open(&dir)?)).await;
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("=== Step 1: start a run and crash ===");
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg(CRASH_FLAG)
        .status()?;
    println!("  child exit: {status}");
    if status.code() == Some(0) {
        println!("  The model did not call the tool, so the run finished. Nothing to resume.");
        return Ok(());
    }
    if status.code() != Some(CRASH_EXIT) {
        return Err("the child failed before the simulated crash".into());
    }

    println!(
        "\n=== Step 2: read the partial run from {} ===",
        dir.display()
    );
    let store = Arc::new(JsonlRunStore::open(&dir)?);
    let Some(record) = store.get(REQUEST_ID).await else {
        return Err("the child wrote no run".into());
    };
    for message in &record.messages {
        let text = message.content.as_deref().unwrap_or("<tool call>");
        println!("  {}: {}", message.role, text);
    }

    println!("\n=== Step 3: resume ===");
    let context = resume_context_from_record(&record, ChatOptions::default())?;
    let client = client(store.clone()).await?;
    let outcome = client
        .complete_messages(context.messages, call_options())
        .await?;
    println!("  assistant: {}", outcome.content.unwrap_or_default());

    let finished = store.get(REQUEST_ID).await.map(|r| r.outcome.is_some());
    println!("  run finished in the store: {}", finished.unwrap_or(false));
    Ok(())
}
