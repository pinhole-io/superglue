//! Stream one completion that does not call tools.

use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use std::collections::BTreeMap;
use std::pin::Pin;
use tracing::instrument;

use std::time::Instant;

use crate::events::{ProcessEvent, ProcessEventKind, StatusEmitter, emit_safe};
use crate::guardrails::{GuardrailError, GuardrailOutcome, GuardrailRegistry, GuardrailStage};
use crate::hooks::{HookContext, HookRegistry, HookStage};
use crate::http::{
    Error as HttpError, HttpClient,
    sse::{HeartbeatAction, HeartbeatWatch, SseParser},
};
use crate::openai::{ChatCompletionChunk, ChatMessage, MessageContent, ToolCall};
use crate::proto;
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

use super::dispatch::*;
use super::error::*;
use super::options::*;
use super::provider::*;

// ---------------------------------------------------------------------------
// Streaming chat completion (SSE / stream: true)
// ---------------------------------------------------------------------------

/// Outcome of a streaming completion (no tool-call loop).
#[derive(Debug, Clone)]
pub struct StreamOutcome {
    /// Full accumulated assistant content.
    pub content: String,
    /// `finish_reason` from the final chunk.
    pub finish_reason: Option<String>,
    /// Usage reported by the final chunk (only when `stream_options.include_usage = true`).
    pub usage: Option<proto::Usage>,
    /// Correlation ID for this request (UUID v4 auto-generated if not supplied by caller).
    pub request_id: String,
}

/// Stream a chat completion, calling `on_delta` for each content token as it arrives.
///
/// Unlike [`complete_with_tools`], this function does **not** execute tool calls.
/// Input guardrails run on the last user message before the stream starts; output
/// guardrails run on the fully-accumulated response after the stream ends (no retry —
/// same behaviour as gluellm's simple streaming path).
#[instrument(
    skip(http, hooks, guardrails, messages, options, on_delta),
    fields(model = %options.model)
)]
pub async fn stream_complete<F>(
    http: &HttpClient,
    hooks: &HookRegistry,
    guardrails: &GuardrailRegistry,
    messages: Vec<ChatMessage>,
    options: &ChatOptions,
    mut on_delta: F,
) -> Result<StreamOutcome, ChatError>
where
    F: FnMut(String) + Send,
{
    let start = Instant::now();
    let request_id = options
        .request_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tracing::info!(
        request_id = %request_id,
        model = %options.model,
        "stream_complete started"
    );
    crate::telemetry::openinference::tag_chain(&request_id);

    let mut full_messages: Vec<ChatMessage> = Vec::with_capacity(messages.len() + 1);
    let _injected_prefix = prepend_system_messages(options, &mut full_messages);
    full_messages.extend(messages);
    crate::telemetry::openinference::set_input_text(&last_user_text(&full_messages));

    // --- Input guardrails ---
    if !guardrails.input_is_empty().await {
        let last_user = full_messages
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
                    if let Some(msg) = full_messages.iter_mut().rev().find(|m| m.role == "user") {
                        msg.content = Some(MessageContent::Text(transformed));
                    }
                }
            }
            GuardrailOutcome::Block(reason) => {
                return Err(ChatError::Guardrail(GuardrailError::new(
                    GuardrailStage::Input,
                    guard_name,
                    reason,
                )));
            }
        }
    }

    // --- PreCompletion hook (observation only) ---
    let last_user = full_messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_ref())
        .and_then(|c| c.as_text().map(str::to_string))
        .unwrap_or_default();
    hooks
        .run(
            HookStage::PreCompletion,
            HookContext::new(HookStage::PreCompletion, last_user),
        )
        .await?;

    // Check cancellation before initiating the stream.
    if let Some(token) = &options.cancel {
        if token.is_cancelled() {
            return Err(ChatError::Cancelled);
        }
    }

    let credentials = credentials_for(options);
    let model_ref = crate::providers::parse_model_ref(&options.model);
    credentials
        .key_for(model_ref.provider)
        .map_err(ChatError::Credentials)?;
    let provider = resolve_chat_provider(&model_ref, &credentials)?;
    let request_messages = messages_with_volatile_suffix(options, &full_messages);
    let ctx = crate::providers::ProviderRequestContext {
        model_ref: &model_ref,
        credentials: &credentials,
        messages: &request_messages,
        tools: None,
        chat_tools: None,
        stream: true,
        options,
    };
    let provider_req = provider.build_chat_request(&ctx);
    let header_refs: Vec<(&str, &str)> = provider_req
        .headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    tracing::debug!(%provider_req.url, "stream_complete POST (SSE)");

    let params = oi_params(options);
    let llm = begin_llm(
        &request_id,
        &model_ref.model,
        model_ref.provider.as_str(),
        &request_messages,
        None,
        &params,
    );
    let mut byte_stream = match http
        .post_json_stream_with_headers(
            &provider_req.url,
            &provider_req.body,
            &header_refs,
            Some(provider_req.rate_limit_key),
        )
        .await
    {
        Ok(stream) => stream,
        Err(err) => {
            llm.fail(&err.to_string());
            return Err(ChatError::Http(err));
        }
    };
    tracing::debug!("stream_complete connection established, reading SSE chunks");

    let mut parser = SseParser::new();
    let mut progress = HeartbeatWatch::new(http.config.stream_idle_timeout);
    let mut outcome = StreamOutcome {
        content: String::new(),
        finish_reason: None,
        usage: None,
        request_id: request_id.clone(),
    };
    let mut byte_count = 0usize;
    let mut event_count = 0usize;

    while let Some(chunk) = byte_stream.next().await {
        // Check cancellation between chunks.
        if let Some(token) = &options.cancel {
            if token.is_cancelled() {
                return Err(ChatError::Cancelled);
            }
        }

        let bytes = chunk?;
        byte_count += bytes.len();
        let text = String::from_utf8_lossy(&bytes);
        tracing::trace!(
            chunk_bytes = bytes.len(),
            preview = ?&text[..text.len().min(120)],
            "stream_complete raw chunk"
        );

        let events = parser
            .push_str(&text)
            .map_err(|e| ChatError::Http(HttpError::InvalidJson(e.to_string())))?;
        match progress.observe(&parser, &events, outcome.finish_reason.is_some()) {
            HeartbeatAction::Ignore => continue,
            HeartbeatAction::Finish => break,
            HeartbeatAction::Stall => return Err(heartbeat_stall()),
            HeartbeatAction::Model => {}
        }

        for event in events {
            event_count += 1;
            if event.event.as_deref() == Some("error") {
                return Err(ChatError::Api(format!(
                    "stream error event: {}",
                    event.data.trim()
                )));
            }
            let data = event.data.trim();
            tracing::trace!(
                event_count,
                data_preview = ?&data[..data.len().min(80)],
                "stream_complete SSE event"
            );
            if data == "[DONE]" {
                tracing::trace!("stream_complete received [DONE]");
                break;
            }
            if data.is_empty() {
                continue;
            }
            let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        data_preview = ?&data[..data.len().min(200)],
                        "stream_complete JSON parse error"
                    );
                    return Err(ChatError::Serde(e));
                }
            };

            if let Some(u) = &chunk.usage {
                outcome.usage = Some(crate::usage::usage_from_compat(u));
            }

            for choice in &chunk.choices {
                if let Some(fr) = &choice.finish_reason {
                    tracing::trace!(finish_reason = %fr, "stream_complete finish_reason");
                    outcome.finish_reason = Some(fr.clone());
                }
                if let Some(delta) = &choice.delta.content
                    && !delta.is_empty()
                {
                    tracing::trace!(delta_len = delta.len(), "stream_complete delta");
                    outcome.content.push_str(delta);
                    on_delta(delta.clone());
                }
                if let Some(delta) = choice.delta.reasoning_text() {
                    emit_reasoning_delta(
                        options.status_emitter.as_ref(),
                        &outcome.request_id,
                        &options.model,
                        1,
                        &delta,
                    )
                    .await;
                }
            }
        }
    }

    finish_llm_round(
        &llm,
        &model_ref,
        &outcome.content,
        &[],
        outcome.finish_reason.as_deref(),
        outcome.usage.as_ref(),
    );

    // --- Output guardrails (no retry for streaming) ---
    if !guardrails.output_is_empty().await {
        let (out_outcome, guard_name) = guardrails.run_output(&outcome.content).await;
        match out_outcome {
            GuardrailOutcome::Allow(transformed) => {
                outcome.content = transformed;
            }
            GuardrailOutcome::Block(reason) => {
                return Err(ChatError::Guardrail(GuardrailError::new(
                    GuardrailStage::Output,
                    guard_name,
                    reason,
                )));
            }
        }
    }

    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    metrics::histogram!(crate::telemetry::metrics::STREAM_DURATION_MS, "model" => options.model.clone()).record(elapsed_ms);

    tracing::info!(
        request_id = %outcome.request_id,
        model = %options.model,
        byte_count,
        event_count,
        content_chars = outcome.content.len(),
        elapsed_ms,
        "stream_complete finished"
    );
    crate::telemetry::openinference::set_output_text(&outcome.content);

    Ok(outcome)
}

pub(crate) fn push_early_stream_tool<'a>(
    tc: ToolCall,
    in_flight: &mut FuturesUnordered<
        Pin<
            Box<
                dyn std::future::Future<Output = Result<(String, ChatMessage), ChatError>>
                    + Send
                    + 'a,
            >,
        >,
    >,
    dispatched_ids: &mut BTreeMap<String, ()>,
    ctx: DispatchCtx<'a>,
) {
    if dispatched_ids.contains_key(&tc.id) {
        return;
    }
    dispatched_ids.insert(tc.id.clone(), ());
    let tool_id = tc.id.clone();
    in_flight.push(Box::pin(async move {
        let msg = dispatch_one(&tc, ctx).await?;
        Ok((tool_id, msg))
    }));
}

pub(crate) fn heartbeat_stall() -> ChatError {
    tracing::warn!("LLM stream heartbeats carried no model tokens");
    ChatError::Http(HttpError::StreamTimeout {
        phase: crate::http::StreamTimeoutPhase::Idle,
    })
}

pub(crate) async fn emit_reasoning_delta(
    emitter: Option<&Arc<StatusEmitter>>,
    request_id: &str,
    model: &str,
    round: u32,
    delta: &str,
) {
    if delta.is_empty() {
        return;
    }
    let mut ev = ProcessEvent::new(ProcessEventKind::ReasoningDelta, request_id, model);
    ev.round = round;
    ev.metadata.insert("delta".to_string(), delta.to_string());
    emit_safe(emitter, ev).await;
}
