//! Run one tool call and shape its result.

use super::SharedToolLoopGuard;
use super::file_locks;
use std::sync::Arc;

use serde_json::{Value, json};
use std::collections::HashSet;
use tokio::time::{Duration, sleep};
use tracing::instrument;

use crate::cancel::CancellationToken;
use crate::events::{ProcessEvent, ProcessEventKind, StatusEmitter, emit_safe};
use crate::hooks::{HookContext, HookRegistry, HookStage};
use crate::openai::{ChatMessage, MessageContent, ToolCall};
use crate::tools::{CODE_TOOL_NAME, OnToolError, ToolInvokeError, ToolRegistry, ToolRetryPolicy};
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

use super::error::*;

/// Invoke a tool, retrying on [`ToolInvokeError::HandlerFailed`] according to `policy`.
pub(crate) async fn invoke_with_policy(
    tool: &std::sync::Arc<dyn crate::tools::Tool>,
    name: &str,
    arguments: Value,
    policy: &ToolRetryPolicy,
) -> Result<Value, ToolInvokeError> {
    let result = tool.call(arguments.clone()).await;

    match result {
        Ok(v) => {
            metrics::counter!(crate::telemetry::metrics::TOOL_CALLS_TOTAL, "tool_name" => name.to_string()).increment(1);
            Ok(v)
        }
        Err(ToolInvokeError::HandlerFailed { ref message, .. }) => {
            metrics::counter!(crate::telemetry::metrics::TOOL_CALLS_ERRORS, "tool_name" => name.to_string(), "error_kind" => "handler_failed").increment(1);
            match &policy.on_error {
                OnToolError::FailFast => Err(result.unwrap_err()),
                OnToolError::Skip => {
                    tracing::warn!(tool = name, error = %message, "tool failed (skip policy)");
                    Ok(json!({
                        "ok": false,
                        "error": message,
                    }))
                }
                OnToolError::Retry {
                    max,
                    initial_delay_ms,
                } => {
                    let mut delay = *initial_delay_ms;
                    for attempt in 1..=*max {
                        tracing::warn!(
                            tool = name,
                            attempt,
                            max,
                            delay_ms = delay,
                            "tool failed, retrying"
                        );
                        sleep(Duration::from_millis(delay)).await;
                        delay = delay.saturating_mul(2);

                        match tool.call(arguments.clone()).await {
                            Ok(v) => {
                                metrics::counter!(crate::telemetry::metrics::TOOL_CALLS_TOTAL, "tool_name" => name.to_string()).increment(1);
                                return Ok(v);
                            }
                            Err(ToolInvokeError::HandlerFailed { .. }) if attempt < *max => {
                                continue;
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    Err(result.unwrap_err())
                }
            }
        }
        Err(other) => {
            metrics::counter!(crate::telemetry::metrics::TOOL_CALLS_ERRORS, "tool_name" => name.to_string(), "error_kind" => "other").increment(1);
            Err(other)
        }
    }
}

pub(crate) fn truncate_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

/// Truncate tool-result text before it is appended to chat history.
pub(crate) fn truncate_tool_result(content: String, max_chars: usize) -> String {
    if max_chars == 0 || content.len() <= max_chars {
        return content;
    }
    format!("{}…\n[truncated]", truncate_bytes(&content, max_chars))
}

/// Shared environment for [`dispatch_one`] and early-stream tool dispatch.
#[derive(Clone)]
pub(crate) struct DispatchCtx<'a> {
    pub hooks: &'a HookRegistry,
    pub registry: &'a ToolRegistry,
    pub status_emitter: Option<&'a Arc<StatusEmitter>>,
    pub request_id: &'a str,
    pub round: u32,
    pub model: &'a str,
    pub emit_start: bool,
    pub tool_result_max_chars: usize,
    pub loop_guard: Option<&'a SharedToolLoopGuard>,
    pub cancel: Option<&'a CancellationToken>,
    pub code_allowlist: Option<Arc<HashSet<String>>>,
}

/// Names the model may call. The list is sorted and capped so a bad call stays small.
pub(crate) async fn available_tool_names(registry: &ToolRegistry) -> String {
    const MAX_NAMES: usize = 32;
    let specs = registry.list_specs().await;
    let mut names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return "(none)".to_string();
    }
    if names.len() <= MAX_NAMES {
        return names.join(", ");
    }
    format!("{}, …", names[..MAX_NAMES].join(", "))
}

pub(crate) fn chat_error_class(error: &ChatError) -> &'static str {
    match error.root_cause() {
        ChatError::Http(_) => "http",
        ChatError::Serde(_) => "serialization",
        ChatError::Tool(_) => "tool",
        ChatError::Hook(_) => "hook",
        ChatError::Guardrail(_) => "guardrail",
        ChatError::NoChoice => "no_choice",
        ChatError::EmptyResponse => "empty_response",
        ChatError::MaxToolRounds(_) => "max_tool_rounds",
        ChatError::Cancelled => "cancelled",
        ChatError::Credentials(_) => "credentials",
        ChatError::UnsupportedProvider(_) => "unsupported_provider",
        ChatError::UnsupportedModelCapability(_) => "unsupported_model_capability",
        ChatError::Api(_) => "api",
        ChatError::Object(_) => "object",
        ChatError::PartialTurn { .. } => "partial_turn",
    }
}

/// Dispatch a single `"function"` tool call through the full pre/post hook pipeline.
///
/// Returns the [`ChatMessage`] with `role="tool"` that should be appended to the
/// conversation history. Called concurrently for all tool calls in a single round
/// via [`futures_util::future::join_all`].
#[instrument(
    skip(tc, ctx),
    fields(tool.name = %tc.function.name, tool.id = %tc.id)
)]
pub(crate) async fn dispatch_one(
    tc: &ToolCall,
    ctx: DispatchCtx<'_>,
) -> Result<ChatMessage, ChatError> {
    let DispatchCtx {
        hooks,
        registry,
        status_emitter,
        request_id,
        round,
        model,
        emit_start,
        tool_result_max_chars,
        loop_guard,
        cancel,
        code_allowlist,
    } = ctx;
    let tool_name = tc.function.name.clone();
    crate::telemetry::openinference::tag_tool(request_id, &tool_name, &tc.id);
    if emit_start && let Some(emitter) = status_emitter {
        let mut ev = ProcessEvent::new(ProcessEventKind::ToolCallStart, request_id, model);
        ev.round = round;
        ev.metadata
            .insert("tool_name".to_string(), tool_name.clone());
        ev.metadata
            .insert("tool_call_id".to_string(), tc.id.clone());
        emit_safe(Some(emitter), ev).await;
    }

    let exec_result: Result<String, ChatError> = async {
        let pre_ctx = if hooks.is_empty_for(&HookStage::PreTool).await {
            HookContext::with_meta(
                HookStage::PreTool,
                tc.function.arguments.trim(),
                "tool_name",
                tc.function.name.as_str(),
            )
        } else {
            hooks
                .run(
                    HookStage::PreTool,
                    HookContext::with_meta(
                        HookStage::PreTool,
                        tc.function.arguments.trim(),
                        "tool_name",
                        tc.function.name.as_str(),
                    ),
                )
                .await?
        };
        if pre_ctx.metadata.get("tool_skip").and_then(|v| v.as_bool()) == Some(true) {
            let skip_result = pre_ctx
                .metadata
                .get("tool_skip_result")
                .and_then(|v| v.as_str())
                .unwrap_or("{\"ok\":false,\"error\":\"tool call blocked by hook\"}");
            return Ok(skip_result.to_string());
        }
        // If the LLM's argument JSON was truncated (e.g. by token limits), return a
        // soft error as a tool result so the model can retry rather than killing the turn.
        // An empty arguments string is treated as `{}` because some models omit braces for
        // no-parameter tools.
        let args: Value = {
            let raw = pre_ctx.content.trim();
            if raw.is_empty() {
                Value::Object(Default::default())
            } else {
                match serde_json::from_str(raw) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            tool = %tc.function.name,
                            error = %e,
                            "malformed tool-call JSON — returning error to model"
                        );
                        return Ok(serde_json::to_string(&serde_json::json!({
                            "ok": false,
                            "error": format!("tool call arguments were not valid JSON ({e}). \
                                Please retry with complete, well-formed JSON arguments.")
                        }))?);
                    }
                }
            }
        };
        if tc.function.name == CODE_TOOL_NAME {
            let Some(allowlist) = code_allowlist.as_deref() else {
                return Ok(serde_json::to_string(&json!({
                    "ok": false,
                    "error": "code is only available after request_tools in tool_mode=code"
                }))?);
            };
            #[cfg(not(feature = "code"))]
            {
                let _ = allowlist;
                return Ok(serde_json::to_string(&json!({
                    "ok": false,
                    "error": "tool mode `code` is unavailable in this build; enable SuperGlue feature `code`"
                }))?);
            }
            #[cfg(feature = "code")]
            {
            let source = match source_from_arguments(&args) {
                Ok(source) => source,
                Err(err) => {
                    if cancel.is_some_and(|token| token.is_cancelled()) {
                        return Err(ChatError::Cancelled);
                    }
                    return Ok(serde_json::to_string(&json!({
                        "ok": false,
                        "error": err.to_string(),
                    }))?);
                }
            };
            let value = match crate::tools::execute_code_with_cancel(
                &source,
                allowlist,
                registry,
                CodeLimits::default(),
                cancel,
            )
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    if cancel.is_some_and(|token| token.is_cancelled()) {
                        return Err(ChatError::Cancelled);
                    }
                    return Ok(serde_json::to_string(&json!({
                        "ok": false,
                        "error": err.to_string(),
                    }))?);
                }
            };
            return Ok(serde_json::to_string(&value)?);
            }
        }

        let (tool, policy) = match registry.resolve_invocation(&tc.function.name).await {
            Ok(pair) => pair,
            Err(ToolInvokeError::UnknownTool { name }) => {
                let available = available_tool_names(registry).await;
                tracing::warn!(
                    tool = %name,
                    "unknown tool; the model can choose another tool"
                );
                return Ok(serde_json::to_string(&json!({
                    "ok": false,
                    "error": format!("unknown tool: {name}. Available tools: {available}"),
                }))?);
            }
            Err(err) => return Err(ChatError::Tool(err)),
        };

        if let Some(guard) = loop_guard {
            let mut g = guard.lock().await;
            if let Some(suppressed) = g.check_suppressed(&tc.function.name, &args) {
                return Ok(suppressed);
            }
        }

        // Serialize same-file mutations so concurrent edits in one round cannot
        // invalidate each other's anchors.
        let _file_guard = file_locks::acquire_file_lock(&tc.function.name, &args).await;

        let result = match cancel {
            Some(token) => {
                tokio::select! {
                    biased;
                    () = token.cancelled() => return Err(ChatError::Cancelled),
                    result = invoke_with_policy(
                        &tool,
                        &tc.function.name,
                        args.clone(),
                        &policy,
                    ) => result?,
                }
            }
            None => invoke_with_policy(&tool, &tc.function.name, args.clone(), &policy).await?,
        };
        let result_json = serde_json::to_string(&result)?;

        let mut post_ctx = HookContext::with_meta(
            HookStage::PostTool,
            &result_json,
            "tool_name",
            tc.function.name.as_str(),
        );
        post_ctx.metadata.insert(
            "context_policy".into(),
            json!(tool.context_policy().as_str()),
        );
        post_ctx
            .metadata
            .insert("arguments".into(), json!(tc.function.arguments.trim()));
        let post_ctx = if hooks.is_empty_for(&HookStage::PostTool).await {
            post_ctx
        } else {
            hooks.run(HookStage::PostTool, post_ctx).await?
        };
        let offload_ref = serde_json::from_str::<Value>(&post_ctx.content)
            .ok()
            .and_then(|value| {
                value
                    .get("notepad_ref")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        value
                            .get("notepad_refs")
                            .and_then(Value::as_array)
                            .and_then(|refs| refs.first())
                            .and_then(|reference| reference.get("notepad_ref"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
            });
        if post_ctx.content != result_json && offload_ref.is_some() {
            tracing::info!(
                tool = %tc.function.name,
                source_chars = result_json.chars().count(),
                stub_chars = post_ctx.content.chars().count(),
                entry_id = %offload_ref.as_deref().unwrap_or(""),
                "tool result offloaded to notepad"
            );
        }
        Ok(post_ctx.content)
    }
    .await;

    if let Some(emitter) = status_emitter {
        let mut ev = ProcessEvent::new(ProcessEventKind::ToolCallEnd, request_id, model);
        ev.round = round;
        ev.metadata
            .insert("tool_name".to_string(), tool_name.clone());
        ev.metadata
            .insert("tool_call_id".to_string(), tc.id.clone());
        match &exec_result {
            Ok(_) => {
                ev.metadata.insert("outcome".to_string(), "ok".to_string());
            }
            Err(err) => {
                ev.error_type = Some(chat_error_class(err).to_string());
                ev.metadata
                    .insert("outcome".to_string(), "error".to_string());
            }
        }
        emit_safe(Some(emitter), ev).await;
    }

    if let Err(err) = &exec_result {
        crate::telemetry::openinference::fail_current(chat_error_class(err));
    }

    let content = truncate_tool_result(exec_result?, tool_result_max_chars);
    Ok(ChatMessage {
        role: "tool".to_string(),
        content: Some(MessageContent::Text(content)),
        tool_calls: None,
        tool_call_id: Some(tc.id.clone()),
        name: Some(tool_name),
        refusal: None,
        provider_blocks: None,
    })
}
