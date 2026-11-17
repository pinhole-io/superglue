//! Chat options, prompt blocks, and the completion outcome.

use super::TransitionLog;
use super::reasoning;
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;

use crate::cancel::CancellationToken;
use crate::context::SummarizeContextConfig;
use crate::events::StatusEmitter;
use crate::openai::{ChatMessage, MessageContent, ResponseFormat, StopSequence, ToolChoice};
use crate::proto;
use crate::tools::{ActiveToolSet, ToolMode, ToolRegistry, ToolSpec};
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

pub(crate) async fn refresh_available_tools(
    registry: &ToolRegistry,
    known_specs: &mut Vec<ToolSpec>,
    active_set: &mut ActiveToolSet,
    mode: ToolMode,
) -> bool {
    let available_specs = registry.list_specs().await;
    if available_specs == *known_specs {
        return false;
    }

    *known_specs = available_specs.clone();
    *active_set = ActiveToolSet::new(available_specs, mode);
    true
}

/// Callback for a compact context block appended after a condensed tool round.
#[derive(Clone)]
pub struct ContextBlockProvider(Arc<dyn Fn() -> String + Send + Sync>);

impl ContextBlockProvider {
    pub fn new(provider: impl Fn() -> String + Send + Sync + 'static) -> Self {
        Self(Arc::new(provider))
    }

    pub fn render(&self) -> String {
        (self.0)()
    }
}

impl fmt::Debug for ContextBlockProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContextBlockProvider(..)")
    }
}

/// Callback for low-volume context lifecycle diagnostics.
#[derive(Clone)]
pub struct ContextEventLogger(Arc<dyn Fn(String) + Send + Sync>);

impl ContextEventLogger {
    pub fn new(logger: impl Fn(String) + Send + Sync + 'static) -> Self {
        Self(Arc::new(logger))
    }

    pub fn log(&self, event: impl Into<String>) {
        (self.0)(event.into());
    }
}

impl fmt::Debug for ContextEventLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContextEventLogger(..)")
    }
}

/// One section of the system prompt. Anthropic uses [`SystemPromptBlock::cache`] for
/// `cache_control: { type: "ephemeral" }`; OpenAI/xAI/Groq benefit from stable ordering.
#[derive(Debug, Clone)]
pub struct SystemPromptBlock {
    pub text: String,
    /// When true, Anthropic requests mark this block (and preceding tools) cacheable.
    pub cache: bool,
}

impl SystemPromptBlock {
    #[must_use]
    pub fn cached(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache: true,
        }
    }

    #[must_use]
    pub fn uncached(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache: false,
        }
    }
}

/// Whether [`ChatOptions`] include a configured system prompt (legacy or blocks).
#[must_use]
#[cfg(test)]
pub(crate) fn has_system_prompt(options: &ChatOptions) -> bool {
    options.system_prompt.is_some()
        || options
            .system_prompt_blocks
            .as_ref()
            .is_some_and(|blocks| !blocks.is_empty())
}

/// Concatenated system prompt for diagnostics and Responses `instructions`.
#[must_use]
pub(crate) fn effective_system_prompt(options: &ChatOptions) -> Option<String> {
    if let Some(blocks) = &options.system_prompt_blocks {
        let joined = blocks
            .iter()
            .map(|b| b.text.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        options.system_prompt.clone()
    }
}

/// Count of leading `system` rows [`prepend_system_messages`] will inject.
#[must_use]
pub(crate) fn injected_system_prefix_count(options: &ChatOptions) -> usize {
    if let Some(blocks) = &options.system_prompt_blocks {
        blocks.iter().filter(|b| !b.text.is_empty()).count()
    } else if options
        .system_prompt
        .as_ref()
        .is_some_and(|s| !s.is_empty())
    {
        1
    } else {
        0
    }
}

/// Count of trailing `system` rows [`append_volatile_suffix`] will inject.
#[must_use]
pub(crate) fn injected_system_suffix_count(options: &ChatOptions) -> usize {
    options.volatile_suffix_blocks.as_ref().map_or(0, |blocks| {
        blocks.iter().filter(|b| !b.text.is_empty()).count()
    })
}

/// Prepend configured system blocks. Returns the number of rows injected.
pub(crate) fn prepend_system_messages(
    options: &ChatOptions,
    messages: &mut Vec<ChatMessage>,
) -> usize {
    let before = messages.len();
    if let Some(blocks) = &options.system_prompt_blocks {
        for block in blocks {
            if !block.text.is_empty() {
                messages.push(ChatMessage::text("system", &block.text));
            }
        }
    } else if let Some(sp) = &options.system_prompt
        && !sp.is_empty()
    {
        messages.push(ChatMessage::text("system", sp));
    }
    messages.len() - before
}

/// Append volatile system blocks after the conversation. Returns the number of rows injected.
pub(crate) fn append_volatile_suffix(
    options: &ChatOptions,
    messages: &mut Vec<ChatMessage>,
) -> usize {
    let before = messages.len();
    if let Some(blocks) = &options.volatile_suffix_blocks {
        for block in blocks {
            if !block.text.is_empty() {
                messages.push(ChatMessage::text("system", &block.text));
            }
        }
    }
    messages.len() - before
}

pub(crate) fn messages_with_volatile_suffix<'a>(
    options: &ChatOptions,
    messages: &'a [ChatMessage],
) -> Cow<'a, [ChatMessage]> {
    if injected_system_suffix_count(options) == 0 {
        Cow::Borrowed(messages)
    } else {
        let mut owned = messages.to_vec();
        append_volatile_suffix(options, &mut owned);
        Cow::Owned(owned)
    }
}

#[must_use]
pub fn prompt_prefix_hash(options: &ChatOptions, tool_names: &[&str]) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Some(blocks) = &options.system_prompt_blocks {
        for block in blocks {
            block.text.hash(&mut hasher);
        }
    } else if let Some(sp) = &options.system_prompt {
        sp.hash(&mut hasher);
    }
    for name in tool_names {
        name.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn log_prefix_guard(
    options: &ChatOptions,
    tool_specs: Option<&[crate::tools::ToolSpec]>,
    last_hash: &mut Option<u64>,
) {
    let names: Vec<&str> = tool_specs
        .unwrap_or(&[])
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let hash = prompt_prefix_hash(options, &names);
    if let (Some(logger), Some(prev)) = (&options.context_event_logger, *last_hash)
        && prev != hash
    {
        logger.log(format!(
            "context prefix_hash_changed prev={prev:016x} now={hash:016x} tools={}",
            names.len()
        ));
    }
    *last_hash = Some(hash);
}

/// Snapshot of the full LLM request payload immediately before an HTTP completion call.
#[derive(Debug, Clone)]
pub struct LlmPayloadSnapshot {
    pub round: u32,
    pub request_id: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<ChatMessage>,
}

/// Opt-in observer for diagnostics / forensics (system prompt + messages per round).
#[derive(Clone)]
pub struct LlmPayloadObserver(Arc<dyn Fn(LlmPayloadSnapshot) + Send + Sync>);

impl LlmPayloadObserver {
    pub fn new(observer: impl Fn(LlmPayloadSnapshot) + Send + Sync + 'static) -> Self {
        Self(Arc::new(observer))
    }

    pub fn notify(&self, snapshot: LlmPayloadSnapshot) {
        (self.0)(snapshot);
    }
}

impl fmt::Debug for LlmPayloadObserver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LlmPayloadObserver(..)")
    }
}

pub(crate) fn notify_llm_payload(
    options: &ChatOptions,
    round: u32,
    request_id: &str,
    messages: &[ChatMessage],
) {
    if let Some(observer) = &options.llm_payload_observer {
        observer.notify(LlmPayloadSnapshot {
            round,
            request_id: request_id.to_string(),
            model: options.model.clone(),
            system_prompt: effective_system_prompt(options),
            messages: messages.to_vec(),
        });
    }
}

/// Provider and model settings for [`complete_with_tools`].
///
/// All fields beyond `base_url`, `api_key`, `model`, and `max_tool_rounds` are forwarded
/// directly to the OpenAI `POST /v1/chat/completions` body when set.
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// e.g. `https://api.openai.com` (no trailing slash required).
    pub base_url: String,
    /// API key. Stored as [`secrecy::SecretString`] — never appears in `Debug` output or logs.
    pub api_key: secrecy::SecretString,
    pub model: String,
    /// Maximum **HTTP completion** calls (each can include tool follow-up rounds).
    pub max_tool_rounds: u32,
    /// Prepended as a `"system"` message before all caller messages when set.
    pub system_prompt: Option<String>,
    /// Structured system sections (stable cached prefix first, dynamic tail after).
    /// When set, takes precedence over [`Self::system_prompt`] for message prepending.
    pub system_prompt_blocks: Option<Vec<SystemPromptBlock>>,
    /// Volatile system sections appended after conversation history (todos, memory).
    /// Excluded from returned client messages so they never persist into the next turn.
    pub volatile_suffix_blocks: Option<Vec<SystemPromptBlock>>,
    /// Sticky cache routing key (OpenAI `prompt_cache_key`, xAI Responses).
    pub prompt_cache_key: Option<String>,

    // --- Sampling ---
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub n: Option<u32>,

    // --- Token limits ---
    pub max_completion_tokens: Option<u32>,
    /// Maximum characters per tool-result message in chat history.
    /// `0` keeps the full result. That is the default.
    pub tool_result_max_chars: usize,

    // --- Penalties ---
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,

    // --- Stop sequences ---
    pub stop: Option<StopSequence>,

    // --- Output format ---
    pub response_format: Option<ResponseFormat>,

    // --- Tool control ---
    pub tool_choice: Option<ToolChoice>,
    /// Forwarded to the chat Completions API when set. Omitting (`None`) leaves provider defaults.
    /// **Local** tool execution uses concurrent dispatch regardless — see the `join_all` path in [`complete_with_tools`].
    pub parallel_tool_calls: Option<bool>,

    // --- Logprobs ---
    pub logprobs: Option<bool>,
    pub top_logprobs: Option<u32>,

    // --- Determinism / storage ---
    pub seed: Option<i64>,
    pub store: Option<bool>,

    // --- Service ---
    pub service_tier: Option<String>,

    // --- Reasoning models (o1/o3/o4) ---
    pub reasoning_effort: Option<String>,
    /// Responses API reasoning summary verbosity (`detailed` by default).
    pub reasoning_summary: reasoning::ReasoningSummaryLevel,

    /// JSON object merged into the chat completion request body after typed fields.
    pub extra_json: Option<Value>,

    /// Optional fan-out emitter for typed process events (`llm_call_*`, `tool_call_*`).
    pub status_emitter: Option<Arc<StatusEmitter>>,

    /// Optional observer invoked before each LLM HTTP call with the full message list.
    pub llm_payload_observer: Option<LlmPayloadObserver>,

    // --- Correlation ---
    /// Caller-supplied request identifier used for tracing and correlation.
    /// Auto-generated as a UUID v4 if `None` when the request is executed.
    pub request_id: Option<String>,

    // --- Cooperative cancellation ---
    /// When set, all HTTP calls in this request check the token and return
    /// [`ChatError::Cancelled`] immediately if it has been cancelled.
    pub cancel: Option<CancellationToken>,

    /// Optional ordered model list (primary first). On eligible HTTP failures after retries,
    /// the next model is attempted.
    pub model_fallback: Option<crate::fallback::ModelFallbackChain>,

    /// Multi-provider API keys (when set, used instead of legacy `api_key` / `base_url` alone).
    pub provider_credentials: Option<Arc<crate::providers::ProviderCredentials>>,

    // --- Context optimization (GlueLLM parity) ---
    pub tool_mode: ToolMode,
    pub tool_route_model: Option<String>,
    pub condense_tool_messages: bool,
    /// Warning: when true, tool-round condensing may rewrite the leading system
    /// row and destroy the prompt-cache prefix. Keep off unless you accept a miss.
    pub aaak_tool_condensing: bool,
    pub summarize_context: SummarizeContextConfig,
    pub aaak_compression_enabled: bool,
    pub aaak_compression_model: Option<String>,
    /// Optional compact context block appended after each condensed tool round.
    pub context_block_provider: Option<ContextBlockProvider>,
    /// Optional sink for context lifecycle diagnostics.
    pub context_event_logger: Option<ContextEventLogger>,

    /// Session image cache used to turn tool `vision.image_hash` results into multimodal input.
    pub image_store: Option<Arc<tokio::sync::Mutex<crate::images::ImageStore>>>,
    /// Opt-in sink for replayable steps of the unary tool loops. Audit sets it per call.
    pub transition_log: Option<TransitionLog>,
    /// When the tool-round cap is reached after a tool round, make one call with no tools.
    pub answer_after_tool_cap: bool,
}

impl Default for ChatOptions {
    fn default() -> Self {
        ChatOptions {
            base_url: String::new(),
            api_key: secrecy::SecretString::from(String::new()),
            model: String::new(),
            max_tool_rounds: 0,
            system_prompt: None,
            system_prompt_blocks: None,
            volatile_suffix_blocks: None,
            prompt_cache_key: None,
            temperature: None,
            top_p: None,
            n: None,
            max_completion_tokens: None,
            tool_result_max_chars: 0,
            presence_penalty: None,
            frequency_penalty: None,
            stop: None,
            response_format: None,
            tool_choice: None,
            parallel_tool_calls: None,
            logprobs: None,
            top_logprobs: None,
            seed: None,
            store: None,
            service_tier: None,
            reasoning_effort: None,
            reasoning_summary: reasoning::ReasoningSummaryLevel::default(),
            extra_json: None,
            status_emitter: None,
            llm_payload_observer: None,
            request_id: None,
            cancel: None,
            model_fallback: None,
            provider_credentials: None,
            tool_mode: ToolMode::default(),
            tool_route_model: None,
            condense_tool_messages: false,
            aaak_tool_condensing: false,
            summarize_context: SummarizeContextConfig::default(),
            aaak_compression_enabled: false,
            aaak_compression_model: None,
            context_block_provider: None,
            context_event_logger: None,
            image_store: None,
            transition_log: None,
            answer_after_tool_cap: false,
        }
    }
}

impl ChatOptions {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        ChatOptions {
            base_url: base_url.into(),
            api_key: secrecy::SecretString::from(api_key.into()),
            model: model.into(),
            max_tool_rounds: 16,
            ..Default::default()
        }
    }
}

impl From<proto::ChatOptions> for ChatOptions {
    fn from(p: proto::ChatOptions) -> Self {
        ChatOptions {
            base_url: if p.base_url.is_empty() {
                "https://api.openai.com".to_string()
            } else {
                p.base_url
            },
            api_key: secrecy::SecretString::from(p.api_key),
            model: if p.model.is_empty() {
                "gpt-5.4-nano-2026-03-17-mini".to_string()
            } else {
                p.model
            },
            max_tool_rounds: if p.max_tool_rounds == 0 {
                16
            } else {
                p.max_tool_rounds
            },
            system_prompt: p.system_prompt,
            system_prompt_blocks: None,
            volatile_suffix_blocks: None,
            prompt_cache_key: None,
            temperature: p.temperature,
            top_p: p.top_p,
            n: None,
            max_completion_tokens: p.max_completion_tokens,
            tool_result_max_chars: 0,
            presence_penalty: p.presence_penalty,
            frequency_penalty: p.frequency_penalty,
            stop: p.stop.map(StopSequence::One),
            response_format: None,
            tool_choice: None,
            parallel_tool_calls: p.parallel_tool_calls,
            logprobs: p.logprobs,
            top_logprobs: p.top_logprobs,
            seed: p.seed,
            store: p.store,
            service_tier: p.service_tier,
            reasoning_effort: p.reasoning_effort,
            reasoning_summary: p
                .reasoning_summary
                .as_deref()
                .and_then(reasoning::ReasoningSummaryLevel::parse_str)
                .unwrap_or_default(),
            extra_json: p
                .extra_json
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            status_emitter: None,
            llm_payload_observer: None,
            request_id: None,
            cancel: None,
            model_fallback: None,
            provider_credentials: None,
            tool_mode: parse_tool_mode(p.tool_mode.as_deref()),
            tool_route_model: p.tool_route_model,
            condense_tool_messages: p.condense_tool_messages.unwrap_or(false),
            aaak_tool_condensing: p.aaak_tool_condensing.unwrap_or(false),
            summarize_context: SummarizeContextConfig {
                enabled: p.summarize_context_enabled.unwrap_or(false),
                threshold: usize::try_from(p.summarize_context_threshold.unwrap_or(20))
                    .unwrap_or(20),
                keep_recent: usize::try_from(p.summarize_context_keep_recent.unwrap_or(6))
                    .unwrap_or(12),
                max_chars: 800_000,
                ..SummarizeContextConfig::default()
            },
            aaak_compression_enabled: p.aaak_compression_enabled.unwrap_or(false),
            aaak_compression_model: p.aaak_compression_model,
            context_block_provider: None,
            context_event_logger: None,
            image_store: None,
            transition_log: None,
            answer_after_tool_cap: false,
        }
    }
}

pub(crate) fn parse_tool_mode(s: Option<&str>) -> ToolMode {
    s.map(ToolMode::parse).unwrap_or_default()
}

/// Outcome of a completed turn (assistant returned text or empty after tool loop).
#[derive(Debug, Clone)]
pub struct CompletionOutcome {
    pub content: Option<String>,
    /// Number of completion HTTP calls performed.
    pub rounds: u32,
    /// Token usage reported by the final completion response.
    pub usage: Option<proto::Usage>,
    /// The `finish_reason` from the last choice.
    pub finish_reason: Option<String>,
    /// Correlation ID for this request (UUID v4 auto-generated if not supplied by caller).
    pub request_id: String,
    /// Full caller-visible chat history after this turn (user / assistant / tool roles).
    ///
    /// Excludes synthetic `system` rows injected from [`ChatOptions::system_prompt_blocks`]
    /// or [`ChatOptions::system_prompt`], and trailing [`ChatOptions::volatile_suffix_blocks`],
    /// so this slice can be passed back into [`complete_with_tools`] on the next turn.
    pub messages: Vec<ChatMessage>,
    /// Model that produced the final successful HTTP response (after any fallback).
    pub model_used: String,
}

/// Strips injected leading and trailing `system` rows.
///
/// Drops at most `injected_prefix` leading `system` rows and `injected_suffix`
/// trailing `system` rows. A caller-supplied `system` row after the injected
/// prefix is kept.
#[must_use]
pub fn conversation_messages_for_client(
    messages: &[ChatMessage],
    injected_prefix: usize,
    injected_suffix: usize,
) -> Vec<ChatMessage> {
    let mut start = 0;
    while start < injected_prefix && start < messages.len() && messages[start].role == "system" {
        start += 1;
    }
    let mut end = messages.len();
    let mut stripped = 0;
    while stripped < injected_suffix && end > start && messages[end - 1].role == "system" {
        end -= 1;
        stripped += 1;
    }
    messages[start..end].to_vec()
}

pub(crate) fn terminal_assistant_for_history(
    model_msg: &ChatMessage,
    final_content: &Option<String>,
) -> ChatMessage {
    let model_text = model_msg.content.as_ref().and_then(MessageContent::as_text);
    match final_content {
        Some(t) if model_text != Some(t.as_str()) => {
            let mut m = model_msg.clone();
            m.content = Some(MessageContent::Text(t.clone()));
            m
        }
        _ => model_msg.clone(),
    }
}
