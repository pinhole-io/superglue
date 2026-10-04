//! OpenAI-compatible vector embeddings.

use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::http::{Error as HttpError, HttpClient, join_base_url};
use crate::providers::{
    ProviderCredentials, ProviderId, parse_model_ref, rate_limit_key_for, wire_model_id,
};

/// Input accepted by an embedding model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Text(String),
    Texts(Vec<String>),
}

impl EmbeddingInput {
    fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.trim().is_empty(),
            Self::Texts(texts) => {
                texts.is_empty() || texts.iter().any(|text| text.trim().is_empty())
            }
        }
    }
}

/// Request for one or more vector embeddings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: EmbeddingInput,
    pub dimensions: Option<usize>,
}

/// One vector returned by an embedding model.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding {
    pub index: usize,
    pub vector: Vec<f32>,
}

/// Token usage reported by an embedding provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

/// Embeddings and metadata returned by a provider.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingOutcome {
    pub model: String,
    pub embeddings: Vec<Embedding>,
    pub usage: Option<EmbeddingUsage>,
}

/// Errors from an embedding request.
#[derive(Debug, Error)]
pub enum EmbedError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Credentials(#[from] crate::providers::CredentialsError),
    #[error("unsupported provider for embeddings: {0}")]
    UnsupportedProvider(ProviderId),
    #[error("embedding input must contain non-empty text")]
    EmptyInput,
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error("embedding response did not include data[].embedding")]
    MissingEmbedding,
}

/// Whether a provider exposes an OpenAI-compatible embeddings endpoint.
#[must_use]
pub fn supports_embeddings(provider: ProviderId) -> bool {
    provider.uses_openai_compat()
}

/// Generate vectors with an OpenAI-compatible embedding model.
///
/// The returned vectors keep the dimensions supplied by the provider.
pub async fn embed(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    request: EmbeddingRequest,
) -> Result<EmbeddingOutcome, EmbedError> {
    if request.input.is_empty() {
        return Err(EmbedError::EmptyInput);
    }

    let model_ref = parse_model_ref(&request.model);
    if !supports_embeddings(model_ref.provider) {
        return Err(EmbedError::UnsupportedProvider(model_ref.provider));
    }

    let key = credentials.key_for(model_ref.provider)?;
    let base_url = credentials.base_url_for(model_ref.provider);
    let url = join_base_url(&base_url, "/v1/embeddings");
    let rate_limit_key = rate_limit_key_for(&model_ref, credentials)?;
    let auth = format!("Bearer {}", key.expose_secret());
    let body = WireEmbeddingRequest {
        model: wire_model_id(&model_ref, &base_url, model_ref.provider),
        input: &request.input,
        dimensions: request.dimensions,
    };
    let value = http
        .post_json_with_headers(
            &url,
            &serde_json::to_value(body)?,
            &[("Authorization", auth.as_str())],
            Some(rate_limit_key),
        )
        .await?;

    parse_embedding_response(value)
}

#[derive(Serialize)]
struct WireEmbeddingRequest<'a> {
    model: String,
    input: &'a EmbeddingInput,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct WireEmbeddingResponse {
    #[serde(default)]
    data: Vec<WireEmbedding>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: Option<EmbeddingUsage>,
}

#[derive(Deserialize)]
struct WireEmbedding {
    index: usize,
    embedding: Option<Vec<f32>>,
}

fn parse_embedding_response(value: serde_json::Value) -> Result<EmbeddingOutcome, EmbedError> {
    let response: WireEmbeddingResponse = serde_json::from_value(value)?;
    if response.data.is_empty() {
        return Err(EmbedError::MissingEmbedding);
    }
    let embeddings = response
        .data
        .into_iter()
        .map(|row| {
            Ok(Embedding {
                index: row.index,
                vector: row.embedding.ok_or(EmbedError::MissingEmbedding)?,
            })
        })
        .collect::<Result<Vec<_>, EmbedError>>()?;

    Ok(EmbeddingOutcome {
        model: response.model,
        embeddings,
        usage: response.usage,
    })
}
