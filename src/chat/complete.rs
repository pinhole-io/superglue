//! Run a chat turn until the model stops calling tools.

use std::sync::Arc;

use tracing::{instrument, warn};

use std::time::Instant;

use super::context_ops::{resolve_tool_route, user_context_for_route};
use super::transition;
use super::{
    condense_tool_round, estimate_context_chars, maybe_summarize_messages, new_tool_loop_guard,
};
use crate::costing::apply_resolved_cost_usd;
use crate::events::{ProcessEvent, ProcessEventKind, emit_safe};
use crate::guardrails::{GuardrailError, GuardrailOutcome, GuardrailRegistry, GuardrailStage};
use crate::hooks::{HookRegistry, HookStage};
use crate::http::{Error as HttpError, HttpClient};
use crate::openai::{ChatMessage, MessageContent};
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

// ---------------------------------------------------------------------------
// complete_with_tools
// ---------------------------------------------------------------------------

/// Run OpenAI-style chat completions with tools: calls `POST /v1/chat/completions` until the model
/// returns an assistant message without tool calls or [`ChatOptions::max_tool_rounds`] is exceeded.
///
/// **Input guardrails** run on the last user message before the first LLM call.
/// **Output guardrails** run on the final assistant text with a retry loop
/// (up to [`GuardrailRegistry::max_output_retries`]).
#[instrument(
    skip(http, registry, hooks, guardrails, caller_messages, options),
    fields(model = %options.model)
)]
pub async fn complete_with_tools(
    http: &HttpClient,
    registry: &ToolRegistry,
    hooks: &HookRegistry,
    guardrails: &GuardrailRegistry,
    caller_messages: Vec<ChatMessage>,
    options: &ChatOptions,
) -> Result<CompletionOutcome, ChatError> {
    let start = Instant::now();
    let request_id = options
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tracing::info!(
        request_id = %request_id,
        model = %options.model,
        "complete_with_tools started"
    );
    crate::telemetry::openinference::tag_chain(&request_id);
    metrics::counter!(crate::telemetry::metrics::COMPLETIONS_TOTAL, "model" => options.model.clone()).increment(1);

    let credentials = credentials_for(options);

    // Prepend system prompt if configured.
    let mut messages: Vec<ChatMessage> = Vec::with_capacity(caller_messages.len() + 1);
    let injected_prefix = prepend_system_messages(options, &mut messages);
    messages.extend(caller_messages);
    crate::telemetry::openinference::set_input_text(&last_user_text(&messages));

    // --- Input guardrails: run on the last user message before first LLM call ---
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
    let loop_guard = new_tool_loop_guard();
    let mut last_prefix_hash = None;
    transition::emit_start(
        options.transition_log.as_ref(),
        &request_id,
        &conversation_messages_for_client(&messages, injected_prefix, 0),
    )
    .await;

    let mut all_specs = registry.list_specs().await;
    let mut active_set = ActiveToolSet::new(all_specs.clone(), options.tool_mode);
    let route_model = options
        .tool_route_model
        .as_deref()
        .unwrap_or(DEFAULT_TOOL_ROUTE_MODEL);

    if let Some(token) = &options.cancel
        && token.is_cancelled()
    {
        return Err(ChatError::Cancelled);
    }

    let outcome = loop {
        if api_calls >= options.max_tool_rounds {
            metrics::counter!(crate::telemetry::metrics::COMPLETIONS_ERRORS, "model" => options.model.clone(), "error_kind" => "max_tool_rounds").increment(1);
            return Err(fail_partial(
                ChatError::MaxToolRounds(options.max_tool_rounds),
                &messages,
                injected_prefix,
            ));
        }
        api_calls += 1;
        if let Some(logger) = &options.context_event_logger {
            logger.log(format!(
                "context round={} messages={} chars={}",
                api_calls,
                messages.len(),
                estimate_context_chars(&messages),
            ));
        }
        tracing::info!(
            round = api_calls,
            request_id = %request_id,
            "llm_completion_round"
        );

        // --- PreCompletion hook (observation only) ---
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

        refresh_available_tools(registry, &mut all_specs, &mut active_set, options.tool_mode).await;
        let tool_specs = active_set.specs_for_llm();
        let chat_tools = active_set.chat_tools_for_llm();
        log_prefix_guard(options, tool_specs, &mut last_prefix_hash);
        let request_messages = messages_with_volatile_suffix(options, &messages);
        notify_llm_payload(options, api_calls, &request_id, &request_messages);

        let (val, model_ref) = match provider_chat_post(
            http,
            &credentials,
            &request_messages,
            tool_specs,
            chat_tools,
            options,
            &request_id,
            api_calls,
            false,
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                let mut ev =
                    ProcessEvent::new(ProcessEventKind::LlmCallError, &request_id, &options.model);
                ev.round = api_calls;
                ev.error_type = Some(http_error_type(&e));
                emit_safe(options.status_emitter.as_ref(), ev).await;
                return Err(fail_partial(e, &messages, injected_prefix));
            }
        };
        model_used = model_ref.raw.clone();
        let provider = resolve_chat_provider(&model_ref)
            .map_err(|e| fail_partial(e, &messages, injected_prefix))?;
        let normalized = provider.parse_chat_response(&val).map_err(|e| {
            fail_partial(
                if e.to_string().contains("no choices") {
                    ChatError::NoChoice
                } else {
                    ChatError::Http(HttpError::InvalidJson(e.to_string()))
                },
                &messages,
                injected_prefix,
            )
        })?;

        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: normalized.content.clone().map(MessageContent::Text),
            tool_calls: if normalized.tool_calls.is_empty() {
                None
            } else {
                Some(normalized.tool_calls.clone())
            },
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: normalized.provider_blocks.clone(),
        };

        let tool_call_count = normalized
            .tool_calls
            .iter()
            .filter(|tc| tc.kind == "function")
            .count() as u32;
        let mut usage_proto = normalized.usage.clone();
        if let Some(ref u) = usage_proto {
            if u.prompt_tokens > 0 {
                last_prompt_tokens = Some(u.prompt_tokens);
            }
            accumulated_usage = Some(crate::usage::accumulate_usage(
                accumulated_usage.as_ref(),
                u,
            ));
        }
        let estimated_cost = usage_proto
            .as_mut()
            .map(|u| apply_resolved_cost_usd(&model_used, u, u.cost_usd));

        emit_safe(options.status_emitter.as_ref(), {
            let mut ev = ProcessEvent::new(ProcessEventKind::LlmCallEnd, &request_id, &model_used);
            ev.round = api_calls;
            ev.tool_call_count = tool_call_count;
            ev.usage = usage_proto.clone();
            ev.estimated_cost_usd = estimated_cost;
            ev
        })
        .await;

        // --- PostCompletion hook (observation only) ---
        let assistant_text = normalized.content.clone().unwrap_or_default();
        hooks
            .run(
                HookStage::PostCompletion,
                observation_hook_ctx(
                    HookStage::PostCompletion,
                    assistant_text,
                    &request_id,
                    api_calls,
                    &model_used,
                ),
            )
            .await
            .map_err(|e| fail_partial(ChatError::Hook(e), &messages, injected_prefix))?;

        if !normalized.tool_calls.is_empty() {
            let function_tcs: Vec<_> = normalized
                .tool_calls
                .iter()
                .filter(|tc| tc.kind == "function")
                .collect();

            if active_set.has_router() && is_router_call(&normalized.tool_calls) {
                let query = router_query_from_calls(&normalized.tool_calls)
                    .unwrap_or_else(|| last_user.clone());
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
                let router_call_id = router_call_id_from_calls(&normalized.tool_calls);
                active_set.apply_route(matched);
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
                    let round_start = messages.len();
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
                    transition::emit_messages(
                        options.transition_log.as_ref(),
                        &request_id,
                        &messages[round_start..],
                    )
                    .await;
                }
                continue;
            }

            let round_start = messages.len();
            messages.push(msg.clone());

            // Run tool handlers concurrently on this process (order of `tool` messages follows `tool_calls`).
            tracing::info!(
                count = function_tcs.len(),
                request_id = %request_id,
                "tool_calls_batch"
            );
            let code_allowlist = (active_set.mode() == ToolMode::Code && active_set.routed)
                .then(|| Arc::new(active_set.code_allowlist()));
            let ctx = DispatchCtx {
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
                code_allowlist,
            };
            let results = futures_util::future::join_all(
                function_tcs.iter().map(|tc| dispatch_one(tc, ctx.clone())),
            )
            .await;
            for r in results {
                messages.push(r.map_err(|e| fail_partial(e, &messages, injected_prefix))?);
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
            transition::emit_messages(
                options.transition_log.as_ref(),
                &request_id,
                &messages[round_start..],
            )
            .await;
            continue;
        }

        // --- Terminal response: extract content ---
        let content_empty = normalized
            .content
            .as_ref()
            .is_none_or(|s| s.trim().is_empty());
        if content_empty {
            if empty_round_retries < 1 {
                empty_round_retries += 1;
                // Don't burn a max_tool_rounds slot on a blank provider response.
                api_calls = api_calls.saturating_sub(1);
                tracing::warn!(
                    request_id = %request_id,
                    model = %model_used,
                    finish_reason = ?normalized.finish_reason,
                    "empty LLM round (no content, no tool calls) — retrying once"
                );
                continue;
            }
            tracing::warn!(
                request_id = %request_id,
                model = %model_used,
                finish_reason = ?normalized.finish_reason,
                "empty LLM round after retry — failing turn"
            );
            return Err(fail_partial(
                ChatError::EmptyResponse,
                &messages,
                injected_prefix,
            ));
        }

        let usage = accumulated_usage.clone();
        let content = normalized.content.clone();
        let finish_reason = normalized.finish_reason.clone();

        // --- Output guardrails: retry loop ---
        if guardrails.output_is_empty().await {
            messages.push(msg.clone());
            let client_messages = conversation_messages_for_client(&messages, injected_prefix, 0);
            break CompletionOutcome {
                content,
                rounds: api_calls,
                usage,
                finish_reason,
                request_id: request_id.clone(),
                messages: client_messages,
                model_used: model_used.clone(),
            };
        }

        let text = content.clone().unwrap_or_default();
        let saved_content = content;
        let saved_finish = finish_reason;
        let max_retries = guardrails.max_output_retries;

        let mut final_outcome = None;
        for attempt in 0..=max_retries {
            let (outcome, guard_name) = guardrails.run_output(&text).await;
            match outcome {
                GuardrailOutcome::Allow(transformed) => {
                    let final_content = if transformed == text {
                        saved_content.clone()
                    } else {
                        Some(transformed)
                    };
                    messages.push(terminal_assistant_for_history(&msg, &final_content));
                    let client_messages =
                        conversation_messages_for_client(&messages, injected_prefix, 0);
                    final_outcome = Some(CompletionOutcome {
                        content: final_content,
                        rounds: api_calls,
                        usage,
                        finish_reason: saved_finish.clone(),
                        request_id: request_id.clone(),
                        messages: client_messages,
                        model_used: model_used.clone(),
                    });
                    break;
                }
                GuardrailOutcome::Block(reason) => {
                    if !output_block_should_regen(http, options, &text, &reason).await {
                        warn!(
                            guard = %guard_name,
                            "output guardrail block skipped regen"
                        );
                        messages.push(terminal_assistant_for_history(&msg, &saved_content));
                        let client_messages =
                            conversation_messages_for_client(&messages, injected_prefix, 0);
                        final_outcome = Some(CompletionOutcome {
                            content: saved_content.clone(),
                            rounds: api_calls,
                            usage,
                            finish_reason: saved_finish.clone(),
                            request_id: request_id.clone(),
                            messages: client_messages,
                            model_used: model_used.clone(),
                        });
                        break;
                    }
                    if attempt < max_retries {
                        messages.push(msg.clone());
                        messages.push(ChatMessage::text(
                            "user",
                            format!(
                                "Your previous response was rejected by a content policy ({reason}). \
                                 Please revise it."
                            ),
                        ));
                        if api_calls >= options.max_tool_rounds {
                            metrics::counter!(crate::telemetry::metrics::COMPLETIONS_ERRORS, "model" => options.model.clone(), "error_kind" => "guardrail").increment(1);
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
                        api_calls += 1;
                        let credentials = credentials_for(options);
                        let request_messages = messages_with_volatile_suffix(options, &messages);
                        let (val, model_ref) = provider_chat_post(
                            http,
                            &credentials,
                            &request_messages,
                            None,
                            None,
                            options,
                            &request_id,
                            api_calls,
                            false,
                        )
                        .await
                        .map_err(|e| fail_partial(e, &messages, injected_prefix))?;
                        model_used = model_ref.raw.clone();
                        let provider = resolve_chat_provider(&model_ref)
                            .map_err(|e| fail_partial(e, &messages, injected_prefix))?;
                        let normalized = provider.parse_chat_response(&val).map_err(|e| {
                            fail_partial(
                                ChatError::Http(HttpError::InvalidJson(e.to_string())),
                                &messages,
                                injected_prefix,
                            )
                        })?;
                        let retry_text = normalized.content.clone().unwrap_or_default();
                        let (out2, gn2) = guardrails.run_output(&retry_text).await;
                        match out2 {
                            GuardrailOutcome::Allow(t) => {
                                let retry_msg = ChatMessage {
                                    role: "assistant".to_string(),
                                    content: Some(MessageContent::Text(retry_text.clone())),
                                    tool_calls: None,
                                    tool_call_id: None,
                                    name: None,
                                    refusal: None,
                                    provider_blocks: None,
                                };
                                messages.push(terminal_assistant_for_history(
                                    &retry_msg,
                                    &Some(t.clone()),
                                ));
                                let client_messages =
                                    conversation_messages_for_client(&messages, injected_prefix, 0);
                                final_outcome = Some(CompletionOutcome {
                                    content: Some(t),
                                    rounds: api_calls,
                                    usage,
                                    finish_reason: saved_finish.clone(),
                                    request_id: request_id.clone(),
                                    messages: client_messages,
                                    model_used: model_used.clone(),
                                });
                                break;
                            }
                            GuardrailOutcome::Block(r2) => {
                                metrics::counter!(crate::telemetry::metrics::COMPLETIONS_ERRORS, "model" => options.model.clone(), "error_kind" => "guardrail").increment(1);
                                return Err(fail_partial(
                                    ChatError::Guardrail(GuardrailError::new(
                                        GuardrailStage::Output,
                                        gn2,
                                        r2,
                                    )),
                                    &messages,
                                    injected_prefix,
                                ));
                            }
                        }
                    } else {
                        metrics::counter!(crate::telemetry::metrics::COMPLETIONS_ERRORS, "model" => options.model.clone(), "error_kind" => "guardrail").increment(1);
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
        }

        let default_messages = {
            if final_outcome.is_none() {
                messages.push(msg.clone());
            }
            conversation_messages_for_client(&messages, injected_prefix, 0)
        };
        break final_outcome.unwrap_or(CompletionOutcome {
            content: saved_content,
            rounds: api_calls,
            usage,
            finish_reason: saved_finish,
            request_id: request_id.clone(),
            messages: default_messages,
            model_used: model_used.clone(),
        });
    };

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    metrics::histogram!(crate::telemetry::metrics::COMPLETION_DURATION_MS, "model" => outcome.model_used.clone()).record(elapsed_ms);
    tracing::info!(
        request_id = %outcome.request_id,
        model = %outcome.model_used,
        rounds = outcome.rounds,
        elapsed_ms,
        "complete_with_tools finished"
    );
    crate::telemetry::openinference::set_output_text(outcome.content.as_deref().unwrap_or(""));

    Ok(outcome)
}
