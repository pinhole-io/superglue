//! Chat completion proxy: request mapping and upstream forwarding.

use std::sync::Arc;

use axum::http::StatusCode;
use futures_util::StreamExt;
use secrecy::SecretString;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::chat::{ChatError, ChatOptions, proxy_chat_post, proxy_chat_stream};
use crate::costing::{apply_resolved_cost_usd, fetch_generation_cost_usd};
use crate::gateway::GatewayState;
use crate::gateway::auth::AuthContext;
use crate::gateway::budget::check_budget;
use crate::gateway::db::Database;
use crate::gateway::error::{GatewayError, GatewayResult};
use crate::gateway::model_access;
use crate::gateway::proxy_provider::{self, ProxyTarget};
use crate::http::{HttpClient, sse::SseParser};
use crate::openai::{ChatCompletionChunk, ChatCompletionRequest, ChatMessage};
use crate::proto;
use crate::providers::ProviderId;
use crate::responses::{self, ResponseError};
use crate::tools::ToolSpec;
use serde_json::json;

#[cfg(feature = "capture")]
use crate::gateway::capture::{CaptureApi, CaptureSink, CaptureTap, CaptureTapContext};

/// Bounded buffer for gateway SSE proxy streams (matches gRPC stream channel size).
const GATEWAY_STREAM_BUFFER: usize = 64;

#[cfg(feature = "capture")]
fn capture_max_bytes(capture: &Option<Arc<CaptureSink>>) -> usize {
    capture
        .as_ref()
        .map(|sink| sink.max_response_bytes())
        .unwrap_or(0)
}

struct ExtractedUsage {
    usage: proto::Usage,
    generation_id: Option<String>,
}

fn extract_openai_chat_usage(data: &str) -> Option<ExtractedUsage> {
    if !data.contains("\"usage\"") {
        return None;
    }
    let chunk: ChatCompletionChunk = serde_json::from_str(data).ok()?;
    let usage = chunk.usage.as_ref().map(usage_from_compat)?;
    let generation_id = (!chunk.id.is_empty()).then_some(chunk.id);
    Some(ExtractedUsage {
        usage,
        generation_id,
    })
}

fn extract_openai_responses_usage(data: &str) -> Option<ExtractedUsage> {
    if !data.contains("\"usage\"") {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    let usage = v
        .pointer("/response/usage")
        .map(usage_from_responses_json)?;
    let generation_id = v
        .pointer("/response/id")
        .or_else(|| v.get("id"))
        .and_then(|id| id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    Some(ExtractedUsage {
        usage,
        generation_id,
    })
}

async fn resolve_proxy_cost(
    http: &HttpClient,
    credentials: &crate::providers::ProviderCredentials,
    provider: ProviderId,
    model: &str,
    usage: &mut proto::Usage,
    generation_id: Option<&str>,
) -> f64 {
    let generation = match generation_id {
        Some(id) => fetch_generation_cost_usd(http, credentials, provider, id).await,
        None => None,
    };
    apply_resolved_cost_usd(model, usage, generation.or(usage.cost_usd))
}

pub(crate) async fn record_usage_async(
    db: &Database,
    key_id: Option<&str>,
    user_id: &str,
    model: &str,
    usage: &proto::Usage,
    cost: f64,
    request_id: &str,
) -> GatewayResult<()> {
    let key_id = key_id.map(str::to_string);
    let user_id = user_id.to_string();
    let model = model.to_string();
    let request_id = request_id.to_string();
    let prompt_tokens = usage.prompt_tokens;
    let completion_tokens = usage.completion_tokens;
    db.run_blocking(move |db| {
        db.record_usage(
            key_id.as_deref(),
            &user_id,
            &model,
            prompt_tokens,
            completion_tokens,
            cost,
            &request_id,
        )
    })
    .await
}

/// Validate model access and budget before proxying (async-safe).
pub async fn preflight_async(
    db: &Database,
    auth: &AuthContext,
    user_id: &str,
    model: &str,
) -> GatewayResult<()> {
    let auth = auth.clone();
    let user_id = user_id.to_string();
    let model = model.to_string();
    db.run_blocking(move |db| preflight(db, &auth, &user_id, &model))
        .await
}

/// Parsed gateway completion request with optional master-key user field.
#[derive(Debug, serde::Deserialize)]
pub struct GatewayCompletionBody {
    #[serde(flatten)]
    pub completion: ChatCompletionRequest,
    /// Required when using the master key; ignored for virtual keys.
    #[serde(default)]
    pub user: Option<String>,
}

/// Parsed gateway Responses request with optional master-key user field.
#[derive(Debug, serde::Deserialize)]
pub struct GatewayResponsesBody {
    pub model: String,
    pub input: Value,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, Value>,
    #[serde(default)]
    pub user: Option<String>,
}

fn response_error_to_gateway(err: ResponseError) -> GatewayError {
    chat_error_to_gateway(err.into())
}

fn chat_options_from_responses(body: &GatewayResponsesBody, request_id: &str) -> ChatOptions {
    let mut options = ChatOptions {
        base_url: "https://api.openai.com".into(),
        api_key: SecretString::from(String::new()),
        model: body.model.clone(),
        max_tool_rounds: 0,
        system_prompt: body.instructions.clone(),
        ..ChatOptions::default()
    };
    options.request_id = Some(request_id.to_string());
    let mut extra = body.extra.clone();
    if let Some(reasoning) = extra.remove("reasoning") {
        if let Some(effort) = reasoning.get("effort").and_then(|e| e.as_str()) {
            options.reasoning_effort = Some(effort.to_string());
        }
        if let Some(summary) = reasoning.get("summary").and_then(|s| s.as_str())
            && let Some(level) = crate::chat::reasoning::ReasoningSummaryLevel::parse_str(summary)
        {
            options.reasoning_summary = level;
        }
    }
    if !extra.is_empty() {
        options.extra_json = Some(Value::Object(
            extra
                .into_iter()
                .collect::<serde_json::Map<String, Value>>(),
        ));
    }
    options
}

fn apply_reasoning_cap(options: &mut ChatOptions, auth: &AuthContext) {
    if auth.is_master {
        return;
    }
    let Some(max) = auth.max_reasoning_effort.as_deref() else {
        return;
    };
    if let Some(ref effort) = options.reasoning_effort
        && let Some(clamped) = crate::chat::reasoning::clamp_reasoning_effort_str(effort, max)
    {
        options.reasoning_effort = Some(clamped);
    }
}

fn messages_from_responses_input(input: &Value) -> Vec<ChatMessage> {
    if let Some(text) = input.as_str() {
        return vec![ChatMessage::text("user", text)];
    }
    if let Some(items) = input.as_array() {
        let mut messages = Vec::new();
        for item in items {
            let Some(typ) = item.get("type").and_then(|t| t.as_str()) else {
                continue;
            };
            match typ {
                "message" => {
                    let role = item.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                    let content = item.get("content").map(|c| {
                        if let Some(s) = c.as_str() {
                            s.to_string()
                        } else {
                            c.to_string()
                        }
                    });
                    if let Some(text) = content {
                        messages.push(ChatMessage::text(role, text));
                    }
                }
                _ => {}
            }
        }
        if !messages.is_empty() {
            return messages;
        }
    }
    vec![ChatMessage::text("user", input.to_string())]
}

fn responses_body_value(body: GatewayResponsesBody) -> Value {
    let GatewayResponsesBody {
        model,
        input,
        instructions,
        stream,
        extra,
        user: _,
    } = body;
    let mut map = serde_json::Map::new();
    map.insert("model".into(), json!(model));
    map.insert("input".into(), input);
    if let Some(instructions) = instructions {
        map.insert("instructions".into(), json!(instructions));
    }
    if stream.unwrap_or(false) {
        map.insert("stream".into(), json!(true));
    }
    for (k, v) in extra {
        map.insert(k, v);
    }
    Value::Object(map)
}

fn usage_from_responses_json(u: &Value) -> proto::Usage {
    let input = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let output = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let total = u
        .get("total_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(u64::from(input) + u64::from(output)) as u32;
    let cached = crate::usage::cached_tokens_from_usage_json(u);
    let mut usage = crate::usage::usage_from_breakdown(crate::usage::UsageBreakdown {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: Some(total),
        cached_tokens: cached,
        reasoning_tokens: None,
    });
    usage.cost_usd = crate::costing::billed_cost_usd_from_usage_json(u);
    usage
}

/// Non-streaming Responses proxy.
pub async fn proxy_response(
    state: &GatewayState,
    auth: &AuthContext,
    body: GatewayResponsesBody,
) -> GatewayResult<(Value, String)> {
    let http = state.http.as_ref();
    let credentials = &state.credentials;
    let db = &state.db;
    #[cfg(feature = "capture")]
    let capture = state.capture_sink();
    let request_id = Uuid::new_v4().to_string();
    let user_id = crate::gateway::auth::resolve_user_id(auth, body.user.as_deref())?;
    let model = body.model.clone();
    #[cfg(feature = "capture")]
    let max_response_bytes = capture_max_bytes(&capture);
    #[cfg(feature = "capture")]
    let mut tap = CaptureTap::begin(CaptureTapContext {
        sink: capture,
        request_id: request_id.clone(),
        user_id: user_id.clone(),
        key_id: auth.key_id.clone(),
        api: CaptureApi::Responses,
        model_requested: model.clone(),
        stream: false,
        max_response_bytes,
    });
    if let Err(err) = preflight_async(db, auth, &user_id, &model).await {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    if proxy_provider::is_proxy_model(&model) {
        let target = match load_proxy(state, &model).await {
            Ok(target) => target,
            Err(err) => {
                #[cfg(feature = "capture")]
                tap.finish_err(err.to_string());
                return Err(err);
            }
        };
        return forward_proxy_response(
            state,
            auth,
            body,
            target,
            request_id,
            user_id,
            #[cfg(feature = "capture")]
            tap,
        )
        .await;
    }

    let mut options = chat_options_from_responses(&body, &request_id);
    apply_reasoning_cap(&mut options, auth);
    let messages = messages_from_responses_input(&body.input);
    let req_body = responses_body_value(body);
    #[cfg(feature = "capture")]
    tap.set_request(&req_body);

    let (val, model_ref) = responses::proxy_responses_post(
        http,
        credentials.as_ref(),
        &messages,
        &req_body,
        &options,
        &request_id,
    )
    .await
    .map_err(|e| {
        #[cfg(feature = "capture")]
        tap.finish_err(e.to_string());
        response_error_to_gateway(e)
    })?;

    #[cfg(feature = "capture")]
    {
        tap.set_model_resolved(model_ref.raw.clone());
        tap.set_response(&val);
    }

    if let Some(usage) = val.get("usage") {
        let mut proto_usage = usage_from_responses_json(usage);
        let generation_id = val.get("id").and_then(|id| id.as_str());
        let cost = resolve_proxy_cost(
            http,
            credentials,
            model_ref.provider,
            model_ref.raw.as_str(),
            &mut proto_usage,
            generation_id,
        )
        .await;
        record_usage_async(
            db,
            auth.key_id.as_deref(),
            &user_id,
            &model_ref.raw,
            &proto_usage,
            cost,
            &request_id,
        )
        .await?;
        #[cfg(feature = "capture")]
        tap.finish(Some(&proto_usage), Some(cost));
    } else {
        #[cfg(feature = "capture")]
        tap.finish(None, None);
    }

    Ok((val, request_id))
}

/// Streaming Responses proxy.
pub async fn proxy_response_stream(
    state: &GatewayState,
    auth: AuthContext,
    body: GatewayResponsesBody,
) -> GatewayResult<
    impl futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static,
> {
    let http = state.http.as_ref();
    let credentials = &state.credentials;
    let db = state.db.clone();
    #[cfg(feature = "capture")]
    let capture = state.capture_sink();
    let request_id = Uuid::new_v4().to_string();
    let user_id = crate::gateway::auth::resolve_user_id(&auth, body.user.as_deref())?;
    let model = body.model.clone();
    #[cfg(feature = "capture")]
    let max_response_bytes = capture_max_bytes(&capture);
    #[cfg(feature = "capture")]
    let mut tap = CaptureTap::begin(CaptureTapContext {
        sink: capture,
        request_id: request_id.clone(),
        user_id: user_id.clone(),
        key_id: auth.key_id.clone(),
        api: CaptureApi::Responses,
        model_requested: model.clone(),
        stream: true,
        max_response_bytes,
    });
    if let Err(err) = preflight_async(&db, &auth, &user_id, &model).await {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    if proxy_provider::is_proxy_model(&model) {
        let target = match load_proxy(state, &model).await {
            Ok(target) => target,
            Err(err) => {
                #[cfg(feature = "capture")]
                tap.finish_err(err.to_string());
                return Err(err);
            }
        };
        return forward_proxy_response_stream(
            state,
            auth,
            body,
            target,
            request_id,
            user_id,
            #[cfg(feature = "capture")]
            tap,
        )
        .await;
    }

    let mut options = chat_options_from_responses(&body, &request_id);
    apply_reasoning_cap(&mut options, &auth);
    let messages = messages_from_responses_input(&body.input);
    let req_body = responses_body_value(body);
    #[cfg(feature = "capture")]
    tap.set_request(&req_body);

    let (byte_stream, model_ref) = responses::proxy_responses_stream(
        http,
        credentials.as_ref(),
        &messages,
        &req_body,
        &options,
        &request_id,
    )
    .await
    .map_err(|e| {
        #[cfg(feature = "capture")]
        tap.finish_err(e.to_string());
        response_error_to_gateway(e)
    })?;

    let key_id = auth.key_id.clone();
    let (tx, rx) = mpsc::channel(GATEWAY_STREAM_BUFFER);
    let is_anthropic = model_ref.provider == ProviderId::Anthropic;
    let model_raw = model_ref.raw.clone();
    let provider = model_ref.provider;
    let http = http.clone();
    let credentials = Arc::clone(credentials);

    tokio::spawn(async move {
        let mut stream = byte_stream;
        let mut parser = SseParser::new();
        let mut round_usage: Option<proto::Usage> = None;
        let mut generation_id: Option<String> = None;
        #[cfg(feature = "capture")]
        tap.set_model_resolved(model_raw.clone());

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    if let Ok(events) = parser.push_str(&text) {
                        for event in events {
                            let data = event.data.trim();
                            #[cfg(feature = "capture")]
                            tap.push_sse(data);
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            if is_anthropic {
                                let _ = crate::providers::anthropic_stream::AnthropicStreamAccumulator::apply_usage_from_sse_data(
                                    &mut round_usage,
                                    data,
                                );
                            } else if let Some(extracted) = extract_openai_responses_usage(data) {
                                if extracted.generation_id.is_some() {
                                    generation_id = extracted.generation_id;
                                }
                                round_usage = Some(extracted.usage);
                            }
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        #[cfg(feature = "capture")]
                        tap.finish(None, None);
                        return;
                    }
                }
                Err(e) => {
                    #[cfg(feature = "capture")]
                    tap.finish_err(e.to_string());
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    return;
                }
            }
        }

        if let Some(mut usage) = round_usage {
            let cost = resolve_proxy_cost(
                &http,
                credentials.as_ref(),
                provider,
                model_raw.as_str(),
                &mut usage,
                generation_id.as_deref(),
            )
            .await;
            let _ = record_usage_async(
                &db,
                key_id.as_deref(),
                &user_id,
                &model_raw,
                &usage,
                cost,
                &request_id,
            )
            .await;
            #[cfg(feature = "capture")]
            tap.finish(Some(&usage), Some(cost));
        } else {
            #[cfg(feature = "capture")]
            tap.finish(None, None);
        }
    });

    Ok(ReceiverStream::new(rx))
}

/// Map an OpenAI-shaped request to [`ChatOptions`] for upstream provider calls.
pub fn chat_options_from_request(req: &ChatCompletionRequest, request_id: &str) -> ChatOptions {
    let mut options = ChatOptions {
        base_url: "https://api.openai.com".into(),
        api_key: SecretString::from(String::new()),
        model: req.model.clone(),
        max_tool_rounds: 0,
        system_prompt: None,
        temperature: req.temperature,
        top_p: req.top_p,
        n: req.n,
        max_completion_tokens: req.max_completion_tokens,
        presence_penalty: req.presence_penalty,
        frequency_penalty: req.frequency_penalty,
        stop: req.stop.clone(),
        response_format: req.response_format.clone(),
        tool_choice: req.tool_choice.clone(),
        parallel_tool_calls: req.parallel_tool_calls,
        logprobs: req.logprobs,
        top_logprobs: req.top_logprobs,
        seed: req.seed,
        store: req.store,
        service_tier: req.service_tier.clone(),
        reasoning_effort: req.reasoning_effort.clone(),
        ..ChatOptions::default()
    };
    options.request_id = Some(request_id.to_string());
    if !req.extra.is_empty() {
        options.extra_json = Some(Value::Object(
            req.extra
                .clone()
                .into_iter()
                .collect::<serde_json::Map<String, Value>>(),
        ));
    }
    options
}

fn tool_specs_from_request(req: &ChatCompletionRequest) -> Option<Vec<ToolSpec>> {
    req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|t| ToolSpec {
                name: t.function.name.clone(),
                parameters_schema: t.function.parameters.clone(),
                description: t.function.description.clone(),
                static_tool: false,
            })
            .collect()
    })
}

fn usage_from_compat(usage: &crate::openai::Usage) -> proto::Usage {
    crate::usage::usage_from_compat(usage)
}

fn usage_from_chat_response(value: &Value) -> Option<ExtractedUsage> {
    let usage = value.get("usage")?;
    let usage = <crate::openai::Usage as serde::Deserialize>::deserialize(usage).ok()?;
    let generation_id = value
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    Some(ExtractedUsage {
        usage: usage_from_compat(&usage),
        generation_id,
    })
}

fn chat_error_to_gateway(err: ChatError) -> GatewayError {
    match err {
        ChatError::Credentials(e) => GatewayError::upstream(e.to_string()),
        ChatError::Http(e) => GatewayError::upstream(e.to_string()),
        ChatError::Api(msg) => GatewayError::upstream(msg),
        ChatError::Serde(e) => GatewayError::upstream(e.to_string()),
        ChatError::UnsupportedProvider(provider) => GatewayError::bad_request(format!(
            "provider {provider} does not support chat completions; use POST /v1/systemone"
        )),
        ChatError::UnsupportedModelCapability(capability) => GatewayError::bad_request(format!(
            "{capability} model cannot be used for chat; use POST /v1/embeddings"
        )),
        other => GatewayError::upstream(other.to_string()),
    }
}

/// Validate model access and budget before proxying.
pub fn preflight(
    db: &Database,
    auth: &AuthContext,
    user_id: &str,
    model: &str,
) -> GatewayResult<()> {
    if !auth.is_master {
        if !model.contains(':') {
            return Err(GatewayError::forbidden(format!(
                "model must use provider:model format (e.g. openai:gpt-4o-mini); got {model}"
            )));
        }
        let patterns = auth.allowed_models.as_deref().unwrap_or(&[]);
        if !model_access::is_allowed(model, patterns) {
            return Err(GatewayError::forbidden(format!(
                "model {model} is not allowed for this API key"
            )));
        }
    }
    match check_budget(db, user_id) {
        Err(GatewayError::WithStatus { status, .. }) if status == StatusCode::NOT_FOUND => Err(
            GatewayError::bad_request(format!("user {user_id} does not exist")),
        ),
        result => result,
    }
}

/// Non-streaming completion proxy.
pub async fn proxy_completion(
    state: &GatewayState,
    auth: &AuthContext,
    body: GatewayCompletionBody,
) -> GatewayResult<(Value, String)> {
    let http = state.http.as_ref();
    let credentials = &state.credentials;
    let db = &state.db;
    #[cfg(feature = "capture")]
    let capture = state.capture_sink();
    let request_id = Uuid::new_v4().to_string();
    let user_id = crate::gateway::auth::resolve_user_id(auth, body.user.as_deref())?;
    #[cfg(feature = "capture")]
    let max_response_bytes = capture_max_bytes(&capture);
    #[cfg(feature = "capture")]
    let mut tap = CaptureTap::begin(CaptureTapContext {
        sink: capture,
        request_id: request_id.clone(),
        user_id: user_id.clone(),
        key_id: auth.key_id.clone(),
        api: CaptureApi::ChatCompletions,
        model_requested: body.completion.model.clone(),
        stream: false,
        max_response_bytes,
    });
    if let Err(err) = preflight_async(db, auth, &user_id, &body.completion.model).await {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    if proxy_provider::is_proxy_model(&body.completion.model) {
        let model = body.completion.model.clone();
        let target = match load_proxy(state, &model).await {
            Ok(target) => target,
            Err(err) => {
                #[cfg(feature = "capture")]
                tap.finish_err(err.to_string());
                return Err(err);
            }
        };
        return forward_proxy_completion(
            state,
            auth,
            body,
            target,
            request_id,
            user_id,
            #[cfg(feature = "capture")]
            tap,
        )
        .await;
    }

    let mut options = chat_options_from_request(&body.completion, &request_id);
    apply_reasoning_cap(&mut options, auth);
    let tool_specs = tool_specs_from_request(&body.completion);
    let tool_refs = tool_specs.as_deref();
    #[cfg(feature = "capture")]
    if let Ok(request) = serde_json::to_value(&body.completion) {
        tap.set_request(&request);
    }

    let (val, model_ref) = proxy_chat_post(
        http,
        credentials.as_ref(),
        &body.completion.messages,
        tool_refs,
        &options,
        &request_id,
    )
    .await
    .map_err(|e| {
        #[cfg(feature = "capture")]
        tap.finish_err(e.to_string());
        chat_error_to_gateway(e)
    })?;

    #[cfg(feature = "capture")]
    {
        tap.set_model_resolved(model_ref.raw.clone());
        tap.set_response(&val);
    }

    if let Some(extracted) = usage_from_chat_response(&val) {
        let mut proto_usage = extracted.usage;
        let cost = resolve_proxy_cost(
            http,
            credentials,
            model_ref.provider,
            model_ref.raw.as_str(),
            &mut proto_usage,
            extracted.generation_id.as_deref(),
        )
        .await;
        record_usage_async(
            db,
            auth.key_id.as_deref(),
            &user_id,
            &model_ref.raw,
            &proto_usage,
            cost,
            &request_id,
        )
        .await?;
        #[cfg(feature = "capture")]
        tap.finish(Some(&proto_usage), Some(cost));
    } else {
        #[cfg(feature = "capture")]
        tap.finish(None, None);
    }

    Ok((val, request_id))
}

/// Streaming completion proxy — forwards SSE bytes and logs usage after the stream ends.
pub async fn proxy_completion_stream(
    state: &GatewayState,
    auth: AuthContext,
    body: GatewayCompletionBody,
) -> GatewayResult<
    impl futures_util::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static,
> {
    let http = state.http.as_ref();
    let credentials = &state.credentials;
    let db = state.db.clone();
    #[cfg(feature = "capture")]
    let capture = state.capture_sink();
    let request_id = Uuid::new_v4().to_string();
    let user_id = crate::gateway::auth::resolve_user_id(&auth, body.user.as_deref())?;
    let model = body.completion.model.clone();
    #[cfg(feature = "capture")]
    let max_response_bytes = capture_max_bytes(&capture);
    #[cfg(feature = "capture")]
    let mut tap = CaptureTap::begin(CaptureTapContext {
        sink: capture,
        request_id: request_id.clone(),
        user_id: user_id.clone(),
        key_id: auth.key_id.clone(),
        api: CaptureApi::ChatCompletions,
        model_requested: model.clone(),
        stream: true,
        max_response_bytes,
    });
    if let Err(err) = preflight_async(&db, &auth, &user_id, &model).await {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    if proxy_provider::is_proxy_model(&model) {
        let target = match load_proxy(state, &model).await {
            Ok(target) => target,
            Err(err) => {
                #[cfg(feature = "capture")]
                tap.finish_err(err.to_string());
                return Err(err);
            }
        };
        return forward_proxy_completion_stream(
            state,
            &auth,
            body,
            target,
            request_id,
            user_id,
            #[cfg(feature = "capture")]
            tap,
        )
        .await;
    }

    let mut options = chat_options_from_request(&body.completion, &request_id);
    apply_reasoning_cap(&mut options, &auth);
    let tool_specs = tool_specs_from_request(&body.completion);
    let tool_refs = tool_specs.as_deref();
    #[cfg(feature = "capture")]
    if let Ok(request) = serde_json::to_value(&body.completion) {
        tap.set_request(&request);
    }

    let (byte_stream, model_ref) = proxy_chat_stream(
        http,
        credentials.as_ref(),
        &body.completion.messages,
        tool_refs,
        &options,
        &request_id,
    )
    .await
    .map_err(|e| {
        #[cfg(feature = "capture")]
        tap.finish_err(e.to_string());
        chat_error_to_gateway(e)
    })?;

    let is_anthropic = model_ref.provider == ProviderId::Anthropic;
    let key_id = auth.key_id.clone();
    let model_raw = model_ref.raw.clone();
    let provider = model_ref.provider;
    let http = http.clone();
    let credentials = Arc::clone(credentials);
    let (tx, rx) = mpsc::channel(GATEWAY_STREAM_BUFFER);

    tokio::spawn(async move {
        let mut stream = byte_stream;
        let mut parser = SseParser::new();
        let mut round_usage: Option<proto::Usage> = None;
        let mut generation_id: Option<String> = None;
        #[cfg(feature = "capture")]
        tap.set_model_resolved(model_raw.clone());

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    if let Ok(events) = parser.push_str(&text) {
                        for event in events {
                            let data = event.data.trim();
                            #[cfg(feature = "capture")]
                            tap.push_sse(data);
                            if data == "[DONE]" {
                                continue;
                            }
                            if is_anthropic {
                                let _ = crate::providers::anthropic_stream::AnthropicStreamAccumulator::apply_usage_from_sse_data(
                                    &mut round_usage,
                                    data,
                                );
                            } else if let Some(extracted) = extract_openai_chat_usage(data) {
                                if extracted.generation_id.is_some() {
                                    generation_id = extracted.generation_id;
                                }
                                round_usage = Some(extracted.usage);
                            }
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        #[cfg(feature = "capture")]
                        tap.finish(None, None);
                        return;
                    }
                }
                Err(e) => {
                    #[cfg(feature = "capture")]
                    tap.finish_err(e.to_string());
                    let _ = tx.send(Err(std::io::Error::other(e.to_string()))).await;
                    return;
                }
            }
        }

        if let Some(mut usage) = round_usage {
            let cost = resolve_proxy_cost(
                &http,
                credentials.as_ref(),
                provider,
                model_raw.as_str(),
                &mut usage,
                generation_id.as_deref(),
            )
            .await;
            let _ = record_usage_async(
                &db,
                key_id.as_deref(),
                &user_id,
                &model_raw,
                &usage,
                cost,
                &request_id,
            )
            .await;
            #[cfg(feature = "capture")]
            tap.finish(Some(&usage), Some(cost));
        } else {
            #[cfg(feature = "capture")]
            tap.finish(None, None);
        }
    });

    Ok(ReceiverStream::new(rx))
}

async fn load_proxy(state: &GatewayState, model: &str) -> GatewayResult<ProxyTarget> {
    let model = model.to_string();
    let master = state.config.master_key_exposed().to_string();
    state
        .db
        .run_blocking(move |db| proxy_provider::load_proxy_target(db, &master, &model))
        .await
}

fn reject_proxy_embedding(target: &ProxyTarget) -> GatewayResult<()> {
    if target.embedding {
        Err(GatewayError::bad_request(
            "embedding model cannot be used for chat; use POST /v1/embeddings",
        ))
    } else {
        Ok(())
    }
}

fn proxy_recorded_cost(usage: &proto::Usage) -> f64 {
    usage.cost_usd.unwrap_or(0.0)
}

async fn post_proxy_json(
    state: &GatewayState,
    target: &ProxyTarget,
    path: &str,
    body: &Value,
) -> GatewayResult<Value> {
    let url = proxy_provider::join_proxy_url(&target.base_url, path);
    let bearer = proxy_provider::authorization_value(target);
    let mut headers = Vec::new();
    if let Some(ref value) = bearer {
        headers.push(("Authorization", value.as_str()));
    }
    state
        .proxy_http
        .post_json_with_headers(&url, body, &headers, None)
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))
}

async fn post_proxy_stream(
    state: &GatewayState,
    target: &ProxyTarget,
    path: &str,
    body: &Value,
) -> GatewayResult<
    impl futures_util::Stream<Item = Result<bytes::Bytes, crate::http::Error>> + Send + 'static,
> {
    let url = proxy_provider::join_proxy_url(&target.base_url, path);
    let bearer = proxy_provider::authorization_value(target);
    let mut headers = Vec::new();
    if let Some(ref value) = bearer {
        headers.push(("Authorization", value.as_str()));
    }
    state
        .proxy_http
        .post_json_stream_with_headers(&url, body, &headers, None)
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))
}

async fn forward_proxy_response(
    state: &GatewayState,
    auth: &AuthContext,
    body: GatewayResponsesBody,
    target: ProxyTarget,
    request_id: String,
    user_id: String,
    #[cfg(feature = "capture")] mut tap: CaptureTap,
) -> GatewayResult<(Value, String)> {
    if let Err(err) = reject_proxy_embedding(&target) {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    let mut upstream = responses_body_value(body);
    proxy_provider::prepare_responses_body(&mut upstream, &target, auth);
    #[cfg(feature = "capture")]
    tap.set_request(&upstream);
    let val = match post_proxy_json(state, &target, "/v1/responses", &upstream).await {
        Ok(val) => val,
        Err(err) => {
            #[cfg(feature = "capture")]
            tap.finish_err(err.to_string());
            return Err(err);
        }
    };
    #[cfg(feature = "capture")]
    {
        tap.set_model_resolved(target.raw.clone());
        tap.set_response(&val);
    }
    if let Some(usage) = val.get("usage") {
        let proto_usage = usage_from_responses_json(usage);
        let cost = proxy_recorded_cost(&proto_usage);
        record_usage_async(
            &state.db,
            auth.key_id.as_deref(),
            &user_id,
            &target.raw,
            &proto_usage,
            cost,
            &request_id,
        )
        .await?;
        #[cfg(feature = "capture")]
        tap.finish(Some(&proto_usage), Some(cost));
    } else {
        #[cfg(feature = "capture")]
        tap.finish(None, None);
    }
    Ok((val, request_id))
}

async fn forward_proxy_completion(
    state: &GatewayState,
    auth: &AuthContext,
    body: GatewayCompletionBody,
    target: ProxyTarget,
    request_id: String,
    user_id: String,
    #[cfg(feature = "capture")] mut tap: CaptureTap,
) -> GatewayResult<(Value, String)> {
    if let Err(err) = reject_proxy_embedding(&target) {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    let mut upstream = serde_json::to_value(&body.completion)
        .map_err(|err| GatewayError::bad_request(err.to_string()))?;
    proxy_provider::prepare_chat_body(&mut upstream, &target, auth);
    #[cfg(feature = "capture")]
    tap.set_request(&upstream);
    let val = match post_proxy_json(state, &target, "/v1/chat/completions", &upstream).await {
        Ok(val) => val,
        Err(err) => {
            #[cfg(feature = "capture")]
            tap.finish_err(err.to_string());
            return Err(err);
        }
    };
    #[cfg(feature = "capture")]
    {
        tap.set_model_resolved(target.raw.clone());
        tap.set_response(&val);
    }
    if let Some(extracted) = usage_from_chat_response(&val) {
        let cost = proxy_recorded_cost(&extracted.usage);
        record_usage_async(
            &state.db,
            auth.key_id.as_deref(),
            &user_id,
            &target.raw,
            &extracted.usage,
            cost,
            &request_id,
        )
        .await?;
        #[cfg(feature = "capture")]
        tap.finish(Some(&extracted.usage), Some(cost));
    } else {
        #[cfg(feature = "capture")]
        tap.finish(None, None);
    }
    Ok((val, request_id))
}

async fn forward_proxy_response_stream(
    state: &GatewayState,
    auth: AuthContext,
    body: GatewayResponsesBody,
    target: ProxyTarget,
    request_id: String,
    user_id: String,
    #[cfg(feature = "capture")] mut tap: CaptureTap,
) -> GatewayResult<ReceiverStream<Result<bytes::Bytes, std::io::Error>>> {
    if let Err(err) = reject_proxy_embedding(&target) {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    let mut upstream = responses_body_value(body);
    proxy_provider::prepare_responses_body(&mut upstream, &target, &auth);
    #[cfg(feature = "capture")]
    tap.set_request(&upstream);
    let byte_stream = match post_proxy_stream(state, &target, "/v1/responses", &upstream).await {
        Ok(stream) => stream,
        Err(err) => {
            #[cfg(feature = "capture")]
            tap.finish_err(err.to_string());
            return Err(err);
        }
    };
    Ok(spawn_proxy_sse(
        state.db.clone(),
        byte_stream,
        auth.key_id.clone(),
        user_id,
        target.raw,
        request_id,
        true,
        #[cfg(feature = "capture")]
        tap,
    ))
}

async fn forward_proxy_completion_stream(
    state: &GatewayState,
    auth: &AuthContext,
    body: GatewayCompletionBody,
    target: ProxyTarget,
    request_id: String,
    user_id: String,
    #[cfg(feature = "capture")] mut tap: CaptureTap,
) -> GatewayResult<ReceiverStream<Result<bytes::Bytes, std::io::Error>>> {
    if let Err(err) = reject_proxy_embedding(&target) {
        #[cfg(feature = "capture")]
        tap.finish_err(err.to_string());
        return Err(err);
    }
    let mut upstream = serde_json::to_value(&body.completion)
        .map_err(|err| GatewayError::bad_request(err.to_string()))?;
    proxy_provider::prepare_chat_body(&mut upstream, &target, auth);
    #[cfg(feature = "capture")]
    tap.set_request(&upstream);
    let byte_stream =
        match post_proxy_stream(state, &target, "/v1/chat/completions", &upstream).await {
            Ok(stream) => stream,
            Err(err) => {
                #[cfg(feature = "capture")]
                tap.finish_err(err.to_string());
                return Err(err);
            }
        };
    Ok(spawn_proxy_sse(
        state.db.clone(),
        byte_stream,
        auth.key_id.clone(),
        user_id,
        target.raw,
        request_id,
        false,
        #[cfg(feature = "capture")]
        tap,
    ))
}

fn spawn_proxy_sse(
    db: Database,
    byte_stream: impl futures_util::Stream<Item = Result<bytes::Bytes, crate::http::Error>>
    + Send
    + 'static,
    key_id: Option<String>,
    user_id: String,
    model_raw: String,
    request_id: String,
    responses: bool,
    #[cfg(feature = "capture")] mut tap: CaptureTap,
) -> ReceiverStream<Result<bytes::Bytes, std::io::Error>> {
    let (tx, rx) = mpsc::channel(GATEWAY_STREAM_BUFFER);
    tokio::spawn(async move {
        let mut stream = std::pin::pin!(byte_stream);
        let mut parser = SseParser::new();
        let mut round_usage: Option<proto::Usage> = None;
        #[cfg(feature = "capture")]
        tap.set_model_resolved(model_raw.clone());
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    if let Ok(events) = parser.push_str(&text) {
                        for event in events {
                            let data = event.data.trim();
                            #[cfg(feature = "capture")]
                            tap.push_sse(data);
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            let extracted = if responses {
                                extract_openai_responses_usage(data)
                            } else {
                                extract_openai_chat_usage(data)
                            };
                            if let Some(extracted) = extracted {
                                round_usage = Some(extracted.usage);
                            }
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        #[cfg(feature = "capture")]
                        tap.finish(None, None);
                        return;
                    }
                }
                Err(err) => {
                    #[cfg(feature = "capture")]
                    tap.finish_err(err.to_string());
                    let _ = tx.send(Err(std::io::Error::other(err.to_string()))).await;
                    return;
                }
            }
        }
        if let Some(usage) = round_usage {
            let cost = proxy_recorded_cost(&usage);
            let _ = record_usage_async(
                &db,
                key_id.as_deref(),
                &user_id,
                &model_raw,
                &usage,
                cost,
                &request_id,
            )
            .await;
            #[cfg(feature = "capture")]
            tap.finish(Some(&usage), Some(cost));
        } else {
            #[cfg(feature = "capture")]
            tap.finish(None, None);
        }
    });
    ReceiverStream::new(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::ChatMessage;

    #[test]
    fn maps_chat_options() {
        let req = ChatCompletionRequest::new(
            "openai:gpt-4o-mini".into(),
            vec![ChatMessage::text("user", "hi")],
            None,
        );
        let opts = chat_options_from_request(&req, "req-1");
        assert_eq!(opts.model, "openai:gpt-4o-mini");
        assert_eq!(opts.request_id.as_deref(), Some("req-1"));
    }
}
