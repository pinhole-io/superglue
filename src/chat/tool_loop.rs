//! Stream a completion that can call tools.

use super::stream_tools;
use std::future::Future;
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::pin::Pin;
use tokio::time::{Duration, sleep};
use tracing::instrument;

use std::time::Instant;

use super::context_ops::{resolve_tool_route, user_context_for_route};
use super::{condense_tool_round, maybe_summarize_messages, new_tool_loop_guard};
use crate::costing::apply_resolved_cost_usd;
use crate::events::{ProcessEvent, ProcessEventKind, emit_safe};
use crate::guardrails::{GuardrailError, GuardrailOutcome, GuardrailRegistry, GuardrailStage};
use crate::hooks::{HookRegistry, HookStage};
use crate::http::{
    Error as HttpError, HttpClient,
    sse::{HeartbeatAction, HeartbeatWatch, SseParser},
};
use crate::openai::{ChatCompletionChunk, ChatMessage, MessageContent};
use crate::proto;
use crate::tools::{
    ActiveToolSet, DEFAULT_TOOL_ROUTE_MODEL, ToolMode, ToolRegistry, is_router_call,
    router_call_id_from_calls, router_query_from_calls,
};
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

use super::dispatch::*;
use super::error::*;
use super::options::*;
use super::provider::*;
use super::stream::*;

/// A tool result asks the next round to answer without tools.
///
/// Only the tool messages at the end of the transcript are read.
fn tool_round_closes(messages: &[crate::openai::ChatMessage]) -> bool {
    messages
        .iter()
        .rev()
        .take_while(|message| message.role == "tool")
        .any(|message| {
            message
                .content
                .as_ref()
                .and_then(|content| content.as_text())
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|value| value.get("close_tools").and_then(Value::as_bool))
                .unwrap_or(false)
        })
}

/// Outcome of a streaming completion with tool rounds.
#[derive(Debug, Clone)]
pub struct StreamToolOutcome {
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<proto::Usage>,
    pub request_id: String,
    pub rounds: u32,
    pub model_used: String,
    pub messages: Vec<ChatMessage>,
}

/// Messages supplied after a complete tool round and before the next model call.
pub type RoundBoundary = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<Vec<ChatMessage>, ChatError>> + Send>>
        + Send
        + Sync,
>;

/// Stream a chat completion with multi-round tool execution (SSE).
///
/// Text deltas are forwarded to `on_delta`. Reasoning text goes to
/// `on_reasoning_delta`. Tool rounds mirror [`complete_with_tools`].
#[instrument(
    skip(http, registry, hooks, guardrails, caller_messages, options, on_delta, on_reasoning_delta),
    fields(model = %options.model)
)]
pub async fn stream_complete_with_tools<FO, FR>(
    http: &HttpClient,
    registry: &ToolRegistry,
    hooks: &HookRegistry,
    guardrails: &GuardrailRegistry,
    caller_messages: Vec<ChatMessage>,
    options: &ChatOptions,
    on_delta: FO,
    on_reasoning_delta: FR,
) -> Result<StreamToolOutcome, ChatError>
where
    FO: FnMut(String) + Send,
    FR: FnMut(String) + Send,
{
    stream_complete_with_tools_at_boundary(
        http,
        registry,
        hooks,
        guardrails,
        caller_messages,
        options,
        on_delta,
        on_reasoning_delta,
        None,
    )
    .await
}

/// Stream a tool loop and allow bounded messages at each safe round boundary.
pub async fn stream_complete_with_tools_at_boundary<FO, FR>(
    http: &HttpClient,
    registry: &ToolRegistry,
    hooks: &HookRegistry,
    guardrails: &GuardrailRegistry,
    caller_messages: Vec<ChatMessage>,
    options: &ChatOptions,
    mut on_delta: FO,
    mut on_reasoning_delta: FR,
    round_boundary: Option<RoundBoundary>,
) -> Result<StreamToolOutcome, ChatError>
where
    FO: FnMut(String) + Send,
    FR: FnMut(String) + Send,
{
    let start = Instant::now();
    let request_id = options
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tracing::info!(
        request_id = %request_id,
        model = %options.model,
        "stream_complete_with_tools started"
    );
    crate::telemetry::openinference::tag_chain(&request_id);

    let credentials = credentials_for(options);

    let mut messages: Vec<ChatMessage> = Vec::with_capacity(caller_messages.len() + 1);
    let injected_prefix = prepend_system_messages(options, &mut messages);
    messages.extend(caller_messages);
    crate::telemetry::openinference::set_input_text(&last_user_text(&messages));

    if !guardrails.input_is_empty().await {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.as_ref())
            .and_then(|c| c.as_text().map(str::to_string))
            .unwrap_or_default();
        let (outcome, guard_name) = guardrails.run_input(&last_user).await;
        match outcome {
            GuardrailOutcome::Allow(transformed) => {
                if transformed != last_user {
                    if let Some(msg) = messages.iter_mut().rev().find(|m| m.role == "user") {
                        msg.content = Some(MessageContent::Text(transformed));
                    }
                }
            }
            GuardrailOutcome::Block(reason) => {
                return Err(fail_partial(
                    ChatError::Guardrail(GuardrailError::new(
                        GuardrailStage::Input,
                        guard_name,
                        reason,
                    )),
                    &messages,
                    injected_prefix,
                ));
            }
        }
    }

    let mut api_calls: u32 = 0;
    let mut last_prompt_tokens: Option<u32> = None;
    let mut model_used;
    let mut accumulated_usage: Option<proto::Usage> = None;
    // One automatic re-request when a provider returns a blank round (Groq flake).
    let mut empty_round_retries: u32 = 0;
    // Re-request the same round when the HTTP stream dies before the first token.
    let mut stall_retries: u32 = 0;
    let loop_guard = new_tool_loop_guard();
    let mut last_prefix_hash = None;

    let mut all_specs = registry.list_specs().await;
    let mut active_set = ActiveToolSet::new(all_specs.clone(), options.tool_mode);
    let mut closing = false;
    let mut stream_code_allowlist: Option<Arc<HashSet<String>>> = None;
    let route_model = options
        .tool_route_model
        .as_deref()
        .unwrap_or(DEFAULT_TOOL_ROUTE_MODEL);

    loop {
        if api_calls >= options.max_tool_rounds {
            if options.answer_after_tool_cap && !closing && api_calls > 0 {
                closing = true;
            } else {
                return Err(fail_partial(
                    ChatError::MaxToolRounds(options.max_tool_rounds),
                    &messages,
                    injected_prefix,
                ));
            }
        }
        api_calls += 1;

        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.as_ref())
            .and_then(|c| c.as_text().map(str::to_string))
            .unwrap_or_default();
        hooks
            .run(
                HookStage::PreCompletion,
                observation_hook_ctx(
                    HookStage::PreCompletion,
                    last_user.clone(),
                    &request_id,
                    api_calls,
                    &options.model,
                ),
            )
            .await
            .map_err(|e| fail_partial(ChatError::Hook(e), &messages, injected_prefix))?;

        maybe_summarize_messages(
            http,
            &credentials,
            &mut messages,
            &options.summarize_context,
            options.aaak_compression_enabled,
            options.aaak_compression_model.as_deref(),
            &options.model,
            options,
            &request_id,
            last_prompt_tokens,
        )
        .await
        .map_err(|e| fail_partial(e, &messages, injected_prefix))?;

        emit_safe(options.status_emitter.as_ref(), {
            let mut ev =
                ProcessEvent::new(ProcessEventKind::LlmCallStart, &request_id, &options.model);
            ev.round = api_calls;
            ev
        })
        .await;

        if refresh_available_tools(registry, &mut all_specs, &mut active_set, options.tool_mode)
            .await
        {
            stream_code_allowlist = None;
        }
        let tool_specs = if closing {
            None
        } else {
            active_set.specs_for_llm()
        };
        let chat_tools = if closing {
            None
        } else {
            active_set.chat_tools_for_llm()
        };
        log_prefix_guard(options, tool_specs, &mut last_prefix_hash);
        let request_messages = messages_with_volatile_suffix(options, &messages);
        notify_llm_payload(options, api_calls, &request_id, &request_messages);

        let requested = crate::providers::parse_model_ref(&options.model);
        let params = oi_params(options);
        let llm = begin_llm(
            &request_id,
            &requested.model,
            requested.provider.as_str(),
            &request_messages,
            tool_specs,
            &params,
        );
        let (mut byte_stream, model_ref) = match provider_chat_stream(
            http,
            &credentials,
            &request_messages,
            tool_specs,
            chat_tools,
            options,
            &request_id,
            api_calls,
        )
        .await
        {
            Ok(stream) => stream,
            Err(e) => {
                llm.fail(&e.to_string());
                if should_retry_stalled_stream_round(&e, stall_retries, "", false) {
                    stall_retries += 1;
                    api_calls = api_calls.saturating_sub(1);
                    tracing::warn!(
                        request_id = %request_id,
                        model = %options.model,
                        attempt = stall_retries,
                        error = %e,
                        "LLM stream failed before first token — retrying round"
                    );
                    sleep(Duration::from_millis(400)).await;
                    continue;
                }
                return Err(fail_partial(e, &messages, injected_prefix));
            }
        };
        model_used = model_ref.raw.clone();
        let is_anthropic = model_ref.provider == crate::providers::ProviderId::Anthropic
            && !credentials.uses_gateway();

        let mut parser = SseParser::new();
        let mut progress = HeartbeatWatch::new(http.config.stream_idle_timeout);
        let mut round_content = String::new();
        let mut tool_dispatch = stream_tools::StreamingToolDispatch::new();
        let mut completed_tools: BTreeMap<String, ChatMessage> = BTreeMap::new();
        let mut dispatched_tool_ids: BTreeMap<String, ()> = BTreeMap::new();
        let mut in_flight: FuturesUnordered<
            Pin<
                Box<
                    dyn std::future::Future<Output = Result<(String, ChatMessage), ChatError>>
                        + Send,
                >,
            >,
        > = FuturesUnordered::new();
        let mut dispatch_ctx = DispatchCtx {
            hooks,
            registry,
            status_emitter: options.status_emitter.as_ref(),
            request_id: &request_id,
            round: api_calls,
            model: &options.model,
            emit_start: true,
            tool_result_max_chars: options.tool_result_max_chars,
            loop_guard: Some(&loop_guard),
            cancel: options.cancel.as_ref(),
            code_allowlist: stream_code_allowlist.clone(),
        };
        let mut anthropic_acc =
            crate::providers::anthropic_stream::AnthropicStreamAccumulator::new();
        let mut round_finish: Option<String> = None;
        let mut round_usage: Option<proto::Usage> = None;
        let mut stream_done = false;
        let mut retry_stalled_round = false;

        while !stream_done || !in_flight.is_empty() {
            tokio::select! {
                biased;
                _ = async {
                    if let Some(token) = &options.cancel {
                        token.cancelled().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                }, if options.cancel.is_some() => {
                    return Err(fail_partial(
                        ChatError::Cancelled,
                        &messages,
                        injected_prefix,
                    ));
                }
                result = in_flight.next(), if !in_flight.is_empty() => {
                    match result {
                        Some(Ok((id, msg))) => {
                            completed_tools.insert(id, msg);
                        }
                        Some(Err(e)) => {
                            return Err(fail_partial(e, &messages, injected_prefix));
                        }
                        None => {}
                    }
                }
                chunk = byte_stream.next(), if !stream_done => {
                    let Some(chunk) = chunk else {
                        stream_done = true;
                        continue;
                    };
                    let bytes = match chunk {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            let chat_err = ChatError::from(e);
                            let tools_started = !completed_tools.is_empty()
                                || !dispatched_tool_ids.is_empty()
                                || !in_flight.is_empty();
                            if should_retry_stalled_stream_round(
                                &chat_err,
                                stall_retries,
                                &round_content,
                                tools_started,
                            ) {
                                retry_stalled_round = true;
                                stream_done = true;
                            } else {
                                return Err(fail_partial(
                                    chat_err,
                                    &messages,
                                    injected_prefix,
                                ));
                            }
                            continue;
                        }
                    };
                    let text = String::from_utf8_lossy(&bytes);
                    let events = parser
                        .push_str(&text)
                        .map_err(|e| {
                            fail_partial(
                                ChatError::Http(HttpError::InvalidJson(e.to_string())),
                                &messages,
                                injected_prefix,
                            )
                        })?;
                    match progress.observe(&parser, &events, round_finish.is_some()) {
                        HeartbeatAction::Ignore => continue,
                        HeartbeatAction::Finish => {
                            stream_done = true;
                            continue;
                        }
                        HeartbeatAction::Stall => {
                            let chat_err = heartbeat_stall();
                            let tools_started = !completed_tools.is_empty()
                                || !dispatched_tool_ids.is_empty()
                                || !in_flight.is_empty();
                            if should_retry_stalled_stream_round(
                                &chat_err,
                                stall_retries,
                                &round_content,
                                tools_started,
                            ) {
                                retry_stalled_round = true;
                                stream_done = true;
                            } else {
                                return Err(fail_partial(
                                    chat_err,
                                    &messages,
                                    injected_prefix,
                                ));
                            }
                            continue;
                        }
                        HeartbeatAction::Model => {}
                    }

                    for event in events {
                        if event.event.as_deref() == Some("error") {
                            return Err(fail_partial(
                                ChatError::Api(format!(
                                    "stream error event: {}",
                                    event.data.trim()
                                )),
                                &messages,
                                injected_prefix,
                            ));
                        }
                        let data = event.data.trim();
                        if data == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        if data.is_empty() {
                            continue;
                        }
                        if is_anthropic {
                            if let Some(delta) = anthropic_acc
                                .apply_sse_data(data)
                                .map_err(|e| {
                                    fail_partial(ChatError::Serde(e), &messages, injected_prefix)
                                })?
                            {
                                match delta {
                                    crate::providers::anthropic_stream::AnthropicStreamDelta::Text(
                                        t,
                                    ) => {
                                        round_content.push_str(&t);
                                        on_delta(t);
                                    }
                                    crate::providers::anthropic_stream::AnthropicStreamDelta::Thinking(
                                        t,
                                    ) => {
                                        on_reasoning_delta(t.clone());
                                        emit_reasoning_delta(
                                            options.status_emitter.as_ref(),
                                            &request_id,
                                            &options.model,
                                            api_calls,
                                            &t,
                                        )
                                        .await;
                                    }
                                }
                            }
                            // Anthropic never sends `[DONE]`; stop at `message_stop`
                            // rather than waiting for the socket to close.
                            if crate::providers::anthropic_stream::AnthropicStreamAccumulator::
                                is_terminal_sse_data(data)
                            {
                                stream_done = true;
                                break;
                            }
                        } else {
                            let chunk: ChatCompletionChunk = serde_json::from_str(data)
                                .map_err(|e| {
                                    fail_partial(ChatError::Serde(e), &messages, injected_prefix)
                                })?;
                            for choice in &chunk.choices {
                                if let Some(text) = choice.delta.reasoning_text() {
                                    on_reasoning_delta(text.clone());
                                    emit_reasoning_delta(
                                        options.status_emitter.as_ref(),
                                        &request_id,
                                        &options.model,
                                        api_calls,
                                        &text,
                                    )
                                    .await;
                                }
                            }
                            let prev_len = round_content.len();
                            let ready = stream_tools::apply_openai_chunk(
                                &chunk,
                                &mut round_content,
                                &mut tool_dispatch,
                                &mut round_finish,
                                &mut round_usage,
                            );
                            if round_content.len() > prev_len {
                                on_delta(round_content[prev_len..].to_string());
                            }
                            for tc in ready {
                                push_early_stream_tool(
                                    tc,
                                    &mut in_flight,
                                    &mut dispatched_tool_ids,
                                    dispatch_ctx.clone(),
                                );
                            }
                        }
                    }
                }
            }
        }

        if retry_stalled_round {
            stall_retries += 1;
            api_calls = api_calls.saturating_sub(1);
            tracing::warn!(
                request_id = %request_id,
                model = %options.model,
                attempt = stall_retries,
                "LLM stream stalled before first token — retrying round"
            );
            sleep(Duration::from_millis(400)).await;
            continue;
        }

        if !is_anthropic {
            for tc in tool_dispatch.drain_at_round_end() {
                push_early_stream_tool(
                    tc,
                    &mut in_flight,
                    &mut dispatched_tool_ids,
                    dispatch_ctx.clone(),
                );
            }
            while let Some(result) = in_flight.next().await {
                let (id, msg) = result.map_err(|e| fail_partial(e, &messages, injected_prefix))?;
                completed_tools.insert(id, msg);
            }
        }

        let round = if is_anthropic {
            anthropic_acc.into_round_outcome()
        } else {
            let tool_calls = tool_dispatch.finish_remaining();
            crate::providers::StreamRoundOutcome {
                content: round_content,
                tool_calls,
                finish_reason: round_finish,
                usage: round_usage,
                provider_blocks: None,
            }
        };
        finish_llm_round(
            &llm,
            &model_ref,
            &round.content,
            &round.tool_calls,
            round.finish_reason.as_deref(),
            round.usage.as_ref(),
        );

        let tool_call_count = round
            .tool_calls
            .iter()
            .filter(|tc| tc.kind == "function")
            .count() as u32;
        emit_safe(options.status_emitter.as_ref(), {
            let mut ev = ProcessEvent::new(ProcessEventKind::LlmCallEnd, &request_id, &model_used);
            ev.round = api_calls;
            ev.tool_call_count = tool_call_count;
            ev.usage = round.usage.clone();
            ev.estimated_cost_usd = ev
                .usage
                .as_mut()
                .map(|u| apply_resolved_cost_usd(&model_used, u, u.cost_usd));
            ev
        })
        .await;

        if let Some(ref u) = round.usage {
            if u.prompt_tokens > 0 {
                last_prompt_tokens = Some(u.prompt_tokens);
            }
            accumulated_usage = Some(crate::usage::accumulate_usage(
                accumulated_usage.as_ref(),
                u,
            ));
        }

        hooks
            .run(
                HookStage::PostCompletion,
                observation_hook_ctx(
                    HookStage::PostCompletion,
                    round.content.clone(),
                    &request_id,
                    api_calls,
                    &model_used,
                ),
            )
            .await
            .map_err(|e| fail_partial(ChatError::Hook(e), &messages, injected_prefix))?;

        // Groq (and some weaker models) intermittently finish with stop + empty
        // content and no tool_calls. Retry the same messages once before failing.
        if round.content.trim().is_empty() && round.tool_calls.is_empty() {
            if empty_round_retries < 1 {
                empty_round_retries += 1;
                // Don't burn a max_tool_rounds slot on a blank provider response.
                api_calls = api_calls.saturating_sub(1);
                tracing::warn!(
                    request_id = %request_id,
                    model = %model_used,
                    finish_reason = ?round.finish_reason,
                    "empty LLM round (no content, no tool calls) — retrying once"
                );
                continue;
            }
            tracing::warn!(
                request_id = %request_id,
                model = %model_used,
                finish_reason = ?round.finish_reason,
                "empty LLM round after retry — failing turn"
            );
            return Err(fail_partial(
                ChatError::EmptyResponse,
                &messages,
                injected_prefix,
            ));
        }

        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: if round.content.is_empty() {
                None
            } else {
                Some(MessageContent::Text(round.content.clone()))
            },
            tool_calls: if round.tool_calls.is_empty() {
                None
            } else {
                Some(round.tool_calls.clone())
            },
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: round.provider_blocks.clone(),
        };

        if !closing && !round.tool_calls.is_empty() {
            if active_set.has_router() && is_router_call(&round.tool_calls) {
                let query =
                    router_query_from_calls(&round.tool_calls).unwrap_or_else(|| last_user.clone());
                let user_context = user_context_for_route(&messages, &query);
                let matched = resolve_tool_route(
                    http,
                    &credentials,
                    &user_context,
                    active_set.dynamic_specs(),
                    route_model,
                    options,
                    &request_id,
                )
                .await;
                let catalog = (active_set.mode() == ToolMode::Code)
                    .then(|| active_set.route_catalog(&matched));
                let router_call_id = router_call_id_from_calls(&round.tool_calls);
                active_set.apply_route(matched);
                if active_set.mode() == ToolMode::Code {
                    stream_code_allowlist = Some(Arc::new(active_set.code_allowlist()));
                    dispatch_ctx.code_allowlist = stream_code_allowlist.clone();
                }
                let matched_names: Vec<String> = if active_set.mode() == ToolMode::Code {
                    active_set.code_allowlist().into_iter().collect()
                } else {
                    active_set
                        .specs_for_llm()
                        .unwrap_or(&[])
                        .iter()
                        .filter(|s| !s.static_tool && s.name != crate::tools::ROUTER_TOOL_NAME)
                        .map(|s| s.name.clone())
                        .collect()
                };
                emit_safe(options.status_emitter.as_ref(), {
                    let mut ev =
                        ProcessEvent::new(ProcessEventKind::ToolRoute, &request_id, route_model);
                    ev.round = api_calls;
                    ev.metadata.insert("route_query".to_string(), query.clone());
                    ev.metadata
                        .insert("matched_tools".to_string(), matched_names.join(","));
                    ev
                })
                .await;
                if let Some(catalog) = catalog {
                    messages.push(msg.clone());
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(
                            serde_json::to_string(&catalog).unwrap_or_else(|_| "{}".into()),
                        )),
                        tool_calls: None,
                        tool_call_id: router_call_id,
                        name: Some(crate::tools::ROUTER_TOOL_NAME.to_string()),
                        refusal: None,
                        provider_blocks: None,
                    });
                }
                continue;
            }

            messages.push(msg.clone());

            // If the model's output was cut short by the token limit, the tool-call
            // arguments are truncated and cannot be parsed. Return a soft error for
            // each pending call so the model knows to retry with smaller arguments.
            if round.finish_reason.as_deref() == Some("length") {
                let function_tcs: Vec<_> = round
                    .tool_calls
                    .iter()
                    .filter(|tc| tc.kind == "function")
                    .collect();
                for tc in function_tcs {
                    let error_content = serde_json::to_string(&serde_json::json!({
                        "ok": false,
                        "error": format!(
                            "Your output was cut off by the model's token limit before the \
                             tool-call arguments were complete. The tool '{}' did not run. \
                             Please retry using smaller arguments — write one file at a time, \
                             use edit_file for large changes, or split large content into \
                             multiple smaller write_file calls.",
                            tc.function.name
                        )
                    }))
                    .unwrap_or_else(|_| r#"{"ok":false,"error":"output truncated"}"#.to_string());
                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(MessageContent::Text(error_content)),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        name: Some(tc.function.name.clone()),
                        refusal: None,
                        provider_blocks: None,
                    });
                }
                continue;
            }

            let function_tcs: Vec<_> = round
                .tool_calls
                .iter()
                .filter(|tc| tc.kind == "function")
                .collect();
            if completed_tools.len() == function_tcs.len() && !function_tcs.is_empty() {
                for tc in function_tcs {
                    if let Some(msg) = completed_tools.remove(&tc.id) {
                        messages.push(msg);
                    }
                }
            } else {
                let results = futures_util::future::join_all(
                    function_tcs
                        .iter()
                        .map(|tc| dispatch_one(tc, dispatch_ctx.clone())),
                )
                .await;
                for r in results {
                    messages.push(r.map_err(|e| fail_partial(e, &messages, injected_prefix))?);
                }
            }

            if tool_round_closes(&messages) {
                closing = true;
            }
            if let Some(store) = &options.image_store {
                let store = store.lock().await;
                let _ = crate::images::attach_vision_from_tool_results(&mut messages, &store);
            }

            if options.condense_tool_messages {
                condense_tool_round(
                    &mut messages,
                    options.aaak_tool_condensing,
                    options.context_block_provider.as_ref(),
                    options.context_event_logger.as_ref(),
                );
            }
            if let Some(boundary) = &round_boundary {
                let injected = boundary().await?;
                messages.extend(injected);
            }
            continue;
        }

        let mut final_content = round.content;
        let final_finish = round.finish_reason;
        let final_usage = accumulated_usage;
        messages.push(msg.clone());

        if !guardrails.output_is_empty().await {
            let (out_outcome, guard_name) = guardrails.run_output(&final_content).await;
            match out_outcome {
                GuardrailOutcome::Allow(transformed) => {
                    final_content = transformed;
                }
                GuardrailOutcome::Block(reason) => {
                    return Err(fail_partial(
                        ChatError::Guardrail(GuardrailError::new(
                            GuardrailStage::Output,
                            guard_name,
                            reason,
                        )),
                        &messages,
                        injected_prefix,
                    ));
                }
            }
        }

        let client_messages = conversation_messages_for_client(&messages, injected_prefix, 0);
        let outcome = StreamToolOutcome {
            content: final_content,
            finish_reason: final_finish,
            usage: final_usage,
            request_id: request_id.clone(),
            rounds: api_calls,
            model_used,
            messages: client_messages,
        };

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            request_id = %outcome.request_id,
            model = %outcome.model_used,
            rounds = outcome.rounds,
            elapsed_ms,
            "stream_complete_with_tools finished"
        );
        crate::telemetry::openinference::set_output_text(&outcome.content);
        return Ok(outcome);
    }
}

#[cfg(test)]
mod tests {
    use crate::openai::ChatMessage;

    #[test]
    fn a_finished_agent_result_closes_the_tool_round() {
        let finished = r#"{"finished":true,"close_tools":true,"message":"done"}"#;
        let messages = vec![
            ChatMessage::text("assistant", "checking"),
            ChatMessage::text("tool", r#"{"ok":true}"#),
            ChatMessage::text("tool", finished),
        ];
        assert!(super::tool_round_closes(&messages));
        let earlier = vec![
            ChatMessage::text("tool", finished),
            ChatMessage::text("assistant", "next"),
            ChatMessage::text("tool", r#"{"ok":true}"#),
        ];
        assert!(!super::tool_round_closes(&earlier));
    }
}
