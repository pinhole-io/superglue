//! Provider HTTP calls for one chat round.

use serde_json::Value;

use crate::cancel::CancellationToken;
use crate::hooks::{HookContext, HookStage};
use crate::http::{Error as HttpError, HttpClient};
use crate::openai::{ChatMessage, MessageContent, ToolCall};
use crate::proto;
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

use super::error::*;
use super::options::*;

/// Perform one HTTP JSON POST, honouring an optional cancellation token.
pub(crate) async fn post_json_cancellable(
    http: &HttpClient,
    url: &str,
    body: &Value,
    headers: &[(&str, &str)],
    rate_limit_key: Option<crate::providers::RateLimitKey>,
    cancel: Option<&CancellationToken>,
) -> Result<Value, ChatError> {
    if let Some(token) = cancel {
        tokio::select! {
            biased;
            _ = token.cancelled() => Err(ChatError::Cancelled),
            result = http.post_json_with_headers(url, body, headers, rate_limit_key) => Ok(result?),
        }
    } else {
        Ok(http
            .post_json_with_headers(url, body, headers, rate_limit_key)
            .await?)
    }
}

pub(crate) fn last_user_text(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .and_then(|message| message.content.as_ref())
        .map(MessageContent::text_for_summary)
        .unwrap_or_default()
}

pub(crate) fn oi_params(
    options: &ChatOptions,
) -> crate::telemetry::openinference::InvocationParams<'_> {
    crate::telemetry::openinference::InvocationParams {
        temperature: options.temperature,
        top_p: options.top_p,
        max_completion_tokens: options.max_completion_tokens,
        reasoning_effort: options.reasoning_effort.as_deref(),
        seed: options.seed,
    }
}

pub(crate) fn finish_llm_chat(
    llm: &crate::telemetry::openinference::LlmSpan,
    provider: &dyn crate::providers::LlmProvider,
    model_ref: &crate::providers::ModelRef,
    body: &Value,
) {
    #[cfg(feature = "otlp")]
    {
        match provider.parse_chat_response(body) {
            Ok(normalized) => {
                let output = ChatMessage {
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
                    provider_blocks: None,
                };
                llm.finish(&crate::telemetry::openinference::LlmFinish {
                    model_name: &model_ref.model,
                    provider: model_ref.provider.as_str(),
                    finish_reason: normalized.finish_reason.as_deref(),
                    usage: normalized.usage.as_ref(),
                    output: &output,
                });
            }
            Err(err) => llm.fail(&err.to_string()),
        }
    }
    #[cfg(not(feature = "otlp"))]
    {
        let _ = (llm, provider, model_ref, body);
    }
}

pub(crate) fn finish_llm_round(
    llm: &crate::telemetry::openinference::LlmSpan,
    model_ref: &crate::providers::ModelRef,
    content: &str,
    tool_calls: &[ToolCall],
    finish_reason: Option<&str>,
    usage: Option<&proto::Usage>,
) {
    #[cfg(feature = "otlp")]
    {
        let output = ChatMessage {
            role: "assistant".to_string(),
            content: if content.is_empty() {
                None
            } else {
                Some(MessageContent::Text(content.to_string()))
            },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls.to_vec())
            },
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks: None,
        };
        llm.finish(&crate::telemetry::openinference::LlmFinish {
            model_name: &model_ref.model,
            provider: model_ref.provider.as_str(),
            finish_reason,
            usage,
            output: &output,
        });
    }
    #[cfg(not(feature = "otlp"))]
    {
        let _ = (llm, model_ref, content, tool_calls, finish_reason, usage);
    }
}

pub(crate) fn begin_llm<'a>(
    session_id: &'a str,
    model_name: &'a str,
    provider: &'a str,
    messages: &'a [ChatMessage],
    tools: Option<&'a [crate::tools::ToolSpec]>,
    params: &'a crate::telemetry::openinference::InvocationParams<'a>,
) -> crate::telemetry::openinference::LlmSpan {
    crate::telemetry::openinference::LlmSpan::begin_chat(
        &crate::telemetry::openinference::LlmStart {
            session_id,
            model_name,
            provider,
            messages,
            tools,
            params,
        },
    )
}

/// POST via provider adapter with per-model HTTP retries and optional model fallback chain.
pub(crate) async fn provider_chat_post(
    http: &HttpClient,
    credentials: &crate::providers::ProviderCredentials,
    messages: &[ChatMessage],
    tool_specs: Option<&[crate::tools::ToolSpec]>,
    chat_tools: Option<&[crate::openai::ChatTool]>,
    options: &ChatOptions,
    request_id: &str,
    round: u32,
    stream: bool,
) -> Result<(Value, crate::providers::ModelRef), ChatError> {
    let models = crate::fallback::effective_models(&options.model, options.model_fallback.as_ref());
    let default_policy = crate::fallback::FallbackPolicy::default();
    let policy = options
        .model_fallback
        .as_ref()
        .map(|c| &c.policy)
        .unwrap_or(&default_policy);

    let mut last_err = None;
    for (i, model_str) in models.iter().enumerate() {
        if i > 0 {
            crate::fallback::emit_model_fallback(
                options.status_emitter.as_ref(),
                request_id,
                &models[i - 1],
                model_str,
                round,
            )
            .await;
        }

        let model_ref = crate::providers::parse_model_ref(model_str);
        credentials
            .key_for(model_ref.provider)
            .map_err(ChatError::Credentials)?;

        let provider = resolve_chat_provider(&model_ref)?;
        let ctx = crate::providers::ProviderRequestContext {
            model_ref: &model_ref,
            credentials,
            messages,
            tools: tool_specs,
            chat_tools,
            stream,
            options,
        };
        let req = provider.build_chat_request(&ctx);
        let header_refs: Vec<(&str, &str)> = req
            .headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let params = oi_params(options);
        let llm = begin_llm(
            request_id,
            &model_ref.model,
            model_ref.provider.as_str(),
            messages,
            tool_specs,
            &params,
        );

        match post_json_cancellable(
            http,
            &req.url,
            &req.body,
            &header_refs,
            Some(req.rate_limit_key),
            options.cancel.as_ref(),
        )
        .await
        {
            Ok(v) => {
                finish_llm_chat(&llm, provider.as_ref(), &model_ref, &v);
                return Ok((v, model_ref));
            }
            Err(e) => {
                llm.fail(&e.to_string());
                if i + 1 < models.len()
                    && crate::fallback::chat_error_eligible_for_fallback(&e, policy)
                {
                    last_err = Some(e);
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last_err.unwrap_or(ChatError::Http(HttpError::InvalidJson(
        "model fallback exhausted".into(),
    ))))
}

/// Open a streaming POST via provider adapter with per-model HTTP retries and optional fallback.
pub(crate) async fn provider_chat_stream(
    http: &HttpClient,
    credentials: &crate::providers::ProviderCredentials,
    messages: &[ChatMessage],
    tool_specs: Option<&[crate::tools::ToolSpec]>,
    chat_tools: Option<&[crate::openai::ChatTool]>,
    options: &ChatOptions,
    request_id: &str,
    round: u32,
) -> Result<
    (
        impl futures_util::Stream<Item = Result<bytes::Bytes, HttpError>> + Send + use<>,
        crate::providers::ModelRef,
    ),
    ChatError,
> {
    let models = crate::fallback::effective_models(&options.model, options.model_fallback.as_ref());
    let default_policy = crate::fallback::FallbackPolicy::default();
    let policy = options
        .model_fallback
        .as_ref()
        .map(|c| &c.policy)
        .unwrap_or(&default_policy);

    let mut last_err = None;
    for (i, model_str) in models.iter().enumerate() {
        if i > 0 {
            crate::fallback::emit_model_fallback(
                options.status_emitter.as_ref(),
                request_id,
                &models[i - 1],
                model_str,
                round,
            )
            .await;
        }

        let model_ref = crate::providers::parse_model_ref(model_str);
        credentials
            .key_for(model_ref.provider)
            .map_err(ChatError::Credentials)?;

        let provider = resolve_chat_provider(&model_ref)?;
        let ctx = crate::providers::ProviderRequestContext {
            model_ref: &model_ref,
            credentials,
            messages,
            tools: tool_specs,
            chat_tools,
            stream: true,
            options,
        };
        let req = provider.build_chat_request(&ctx);
        let url = req.url.clone();
        let body = req.body.clone();
        let owned_headers = req.headers.clone();
        let rate_key = req.rate_limit_key;
        let header_refs: Vec<(&str, &str)> = owned_headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        if let Some(token) = options.cancel.as_ref()
            && token.is_cancelled()
        {
            return Err(ChatError::Cancelled);
        }

        match http
            .post_json_stream_with_headers(&url, &body, &header_refs, Some(rate_key))
            .await
        {
            Ok(stream) => return Ok((stream, model_ref)),
            Err(e) => {
                let chat_err = ChatError::Http(e);
                if i + 1 < models.len()
                    && crate::fallback::chat_error_eligible_for_fallback(&chat_err, policy)
                {
                    last_err = Some(chat_err);
                    continue;
                }
                return Err(chat_err);
            }
        }
    }
    Err(last_err.unwrap_or(ChatError::Http(HttpError::InvalidJson(
        "model fallback exhausted".into(),
    ))))
}

/// Gateway-facing wrapper around [`provider_chat_post`].
#[cfg(feature = "gateway")]
pub async fn proxy_chat_post(
    http: &HttpClient,
    credentials: &crate::providers::ProviderCredentials,
    messages: &[ChatMessage],
    tool_specs: Option<&[crate::tools::ToolSpec]>,
    options: &ChatOptions,
    request_id: &str,
) -> Result<(Value, crate::providers::ModelRef), ChatError> {
    provider_chat_post(
        http,
        credentials,
        messages,
        tool_specs,
        None,
        options,
        request_id,
        1,
        false,
    )
    .await
}

/// Gateway-facing wrapper around [`provider_chat_stream`].
#[cfg(feature = "gateway")]
pub async fn proxy_chat_stream(
    http: &HttpClient,
    credentials: &crate::providers::ProviderCredentials,
    messages: &[ChatMessage],
    tool_specs: Option<&[crate::tools::ToolSpec]>,
    options: &ChatOptions,
    request_id: &str,
) -> Result<
    (
        impl futures_util::Stream<Item = Result<bytes::Bytes, HttpError>> + Send + use<>,
        crate::providers::ModelRef,
    ),
    ChatError,
> {
    provider_chat_stream(
        http,
        credentials,
        messages,
        tool_specs,
        None,
        options,
        request_id,
        1,
    )
    .await
}

pub(crate) fn observation_hook_ctx(
    stage: HookStage,
    content: String,
    request_id: &str,
    round: u32,
    model: &str,
) -> HookContext {
    let mut ctx = HookContext::new(stage, content);
    ctx.metadata.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    ctx.metadata
        .insert("round".to_string(), Value::Number(round.into()));
    ctx.metadata
        .insert("model".to_string(), Value::String(model.to_string()));
    ctx
}

pub(crate) fn http_error_type(err: &ChatError) -> String {
    match err.root_cause() {
        ChatError::Http(e) => format!("http:{e}"),
        ChatError::Cancelled => "cancelled".to_string(),
        ChatError::Serde(e) => format!("serde:{e}"),
        ChatError::NoChoice => "no_choice".to_string(),
        other => format!("{other}"),
    }
}
