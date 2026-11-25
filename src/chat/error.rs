//! Chat errors and credential lookup.

use secrecy::ExposeSecret;
use thiserror::Error;
use tokio::time::Duration;

use crate::guardrails::GuardrailError;
use crate::hooks::HookError;
use crate::http::{Error as HttpError, HttpClient};
use crate::openai::ChatMessage;
use crate::tools::ToolInvokeError;
#[cfg(feature = "code")]
use crate::tools::{CodeLimits, source_from_arguments};

use super::options::*;

/// Errors from the chat + tool pipeline.
#[derive(Debug, Error)]
pub enum ChatError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Tool(#[from] ToolInvokeError),
    #[error("hook aborted: {0}")]
    Hook(#[from] HookError),
    #[error(transparent)]
    Guardrail(#[from] GuardrailError),
    #[error("completion response contained no choices")]
    NoChoice,
    /// Provider finished a round with neither assistant text nor tool calls.
    /// Common intermittent failure on weaker models / some Groq tool-calling streams.
    #[error("model returned an empty response (no text, no tool calls)")]
    EmptyResponse,
    #[error("exceeded max tool rounds ({0})")]
    MaxToolRounds(u32),
    #[error("request cancelled")]
    Cancelled,
    #[error(transparent)]
    Credentials(#[from] crate::providers::CredentialsError),
    #[error("unsupported provider for this API: {0}")]
    UnsupportedProvider(crate::providers::ProviderId),
    #[error("{0} model reference cannot be used for chat")]
    UnsupportedModelCapability(crate::providers::ModelCapability),
    #[error("API response failed: {0}")]
    Api(String),
    /// The assistant text was not valid JSON, or it missed the schema.
    /// The string is a short kind and a schema path. It does not include model text.
    #[error("structured output failed: {0}")]
    Object(String),
    /// Tool loop ended before a successful completion; carries transcript for persistence.
    #[error("{cause}")]
    PartialTurn {
        #[source]
        cause: Box<ChatError>,
        messages: Vec<ChatMessage>,
    },
}

impl ChatError {
    /// Client-facing messages accumulated before the failure, if any.
    pub fn partial_messages(&self) -> Option<&[ChatMessage]> {
        match self {
            ChatError::PartialTurn { messages, .. } => Some(messages),
            _ => None,
        }
    }

    /// Inner error when wrapped in [`PartialTurn`]; otherwise `self`.
    pub fn root_cause(&self) -> &ChatError {
        match self {
            ChatError::PartialTurn { cause, .. } => cause.as_ref(),
            other => other,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self.root_cause(), ChatError::Cancelled)
    }

    /// True when the provider closed a stream before any useful tokens.
    /// Common on gateway idle cutoffs (`error decoding response body`) and HTTP timeouts.
    pub fn is_transient_stream_stall(&self) -> bool {
        detail_is_transient_stream_stall(&self.root_cause().to_string())
    }
}

pub(crate) fn resolve_chat_provider(
    model_ref: &crate::providers::ModelRef,
    credentials: &crate::providers::ProviderCredentials,
) -> Result<Box<dyn crate::providers::LlmProvider>, ChatError> {
    if let Some(capability) = model_ref.capability {
        return Err(ChatError::UnsupportedModelCapability(capability));
    }
    crate::providers::resolve_provider_for_credentials(model_ref, credentials)
        .map_err(|err| ChatError::UnsupportedProvider(err.0))
}

/// Shared classifier for stream stalls (timeouts, truncated SSE, connection reset).
pub fn detail_is_transient_stream_stall(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("error decoding response body")
        || detail.contains("timed out")
        || detail.contains("operation timed out")
        || detail.contains("connection reset")
        || detail.contains("connection closed")
        || detail.contains("error sending request")
}

const MAX_STREAM_STALL_RETRIES: u32 = 2;

pub(crate) fn should_retry_stalled_stream_round(
    err: &ChatError,
    stall_retries: u32,
    round_content: &str,
    tools_started: bool,
) -> bool {
    stall_retries < MAX_STREAM_STALL_RETRIES
        && round_content.is_empty()
        && !tools_started
        && err.is_transient_stream_stall()
}

/// Wrap a tool-loop failure with the messages accumulated so far.
pub fn fail_partial(
    cause: ChatError,
    messages: &[ChatMessage],
    injected_prefix: usize,
) -> ChatError {
    ChatError::PartialTurn {
        cause: Box::new(cause),
        messages: conversation_messages_for_client(messages, injected_prefix, 0),
    }
}

/// Resolve credentials from options (multi-provider map or legacy single OpenAI key).
pub fn credentials_for(options: &ChatOptions) -> crate::providers::ProviderCredentials {
    if let Some(creds) = &options.provider_credentials {
        return creds.as_ref().clone();
    }
    let mut creds = crate::providers::ProviderCredentials::new();
    creds.with_legacy_openai_key(options.api_key.expose_secret(), Some(&options.base_url));
    creds
}

const OUTPUT_REVISE_TIMEOUT: Duration = Duration::from_millis(400);
const OUTPUT_REVISE_NOUL: f64 = 0.5;

/// True when an output block should burn another generation.
pub(crate) async fn output_block_should_regen(
    http: &HttpClient,
    options: &ChatOptions,
    text: &str,
    reason: &str,
) -> bool {
    use crate::providers::ProviderId;
    use crate::systemone::{Noul, Questions, SystemOneRequest, clip_state, noul_yes, system_one};

    let creds = credentials_for(options);
    if !creds.has_key(ProviderId::TypeSafe) {
        return true;
    }
    let mut questions = Questions::new();
    questions.insert(
        "regen",
        Noul::new("Is this a clear policy violation that needs another generation?").criteria(
            "Yes. The text must be rewritten.",
            "No. The block is a low-confidence false positive.",
        ),
    );
    let state = format!(
        "Policy reason: {}\n\nAssistant text: {}",
        clip_state(reason, 160),
        clip_state(text, 400)
    );
    let request = SystemOneRequest::new(state, questions).with_model("typesafe:jev-latest");
    let work = system_one(http, &creds, request);
    match tokio::time::timeout(OUTPUT_REVISE_TIMEOUT, work).await {
        Ok(Ok(response)) => response
            .answer("regen")
            .is_none_or(|answer| noul_yes(answer, OUTPUT_REVISE_NOUL)),
        _ => true,
    }
}
