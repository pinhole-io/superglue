//! Vector embeddings.
//!
//! OpenAI-compatible providers use `/v1/embeddings`. Bedrock uses `InvokeModel`.

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
    #[error("unsupported embedding model for {provider}: {model}")]
    UnsupportedModel { provider: ProviderId, model: String },
    #[error("embedding input has {count} texts; the maximum for this model is {max}")]
    TooManyInputs { count: usize, max: usize },
    #[error("invalid embedding model id")]
    InvalidModelId,
}

/// Whether a provider exposes an OpenAI-compatible embeddings endpoint.
#[must_use]
pub fn supports_embeddings(provider: ProviderId) -> bool {
    provider.supports_embeddings()
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
    if model_ref.provider == ProviderId::Bedrock {
        return embed_bedrock(http, credentials, &model_ref, &request).await;
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

async fn embed_bedrock(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    model_ref: &crate::providers::ModelRef,
    request: &EmbeddingRequest,
) -> Result<EmbeddingOutcome, EmbedError> {
    use crate::providers::bedrock::{
        COHERE_MAX_TEXTS, EmbedFamily, TITAN_MAX_TEXTS, cohere_request_body, embed_family,
        invoke_url, parse_cohere_response, parse_titan_response, titan_request_body,
    };

    let family = embed_family(&model_ref.model).map_err(|_| EmbedError::UnsupportedModel {
        provider: ProviderId::Bedrock,
        model: model_ref.model.clone(),
    })?;
    let texts = request.input.texts();
    let max = match family {
        EmbedFamily::TitanText => TITAN_MAX_TEXTS,
        EmbedFamily::Cohere => COHERE_MAX_TEXTS,
    };
    if texts.len() > max {
        return Err(EmbedError::TooManyInputs {
            count: texts.len(),
            max,
        });
    }
    let url = invoke_url(
        &credentials.base_url_for(ProviderId::Bedrock),
        &model_ref.model,
    )
    .map_err(|_| EmbedError::InvalidModelId)?;
    let key = credentials.key_for(ProviderId::Bedrock)?;
    let rate_limit_key = rate_limit_key_for(model_ref, credentials)?;
    let auth = format!("Bearer {}", key.expose_secret());
    let headers = [("Authorization", auth.as_str())];

    let (vectors, prompt_tokens) = match family {
        EmbedFamily::TitanText => {
            let mut vectors = Vec::with_capacity(texts.len());
            let mut prompt_tokens = 0u64;
            let mut saw_tokens = false;
            for text in &texts {
                let value = http
                    .post_json_with_headers(
                        &url,
                        &titan_request_body(text, request.dimensions),
                        &headers,
                        Some(rate_limit_key),
                    )
                    .await?;
                let parsed =
                    parse_titan_response(&value).map_err(|()| EmbedError::MissingEmbedding)?;
                if let Some(tokens) = parsed.input_tokens {
                    prompt_tokens = prompt_tokens.saturating_add(tokens);
                    saw_tokens = true;
                }
                vectors.push(parsed.vector);
            }
            (vectors, saw_tokens.then_some(prompt_tokens))
        }
        EmbedFamily::Cohere => {
            let owned: Vec<String> = texts.iter().map(|text| (*text).to_string()).collect();
            let value = http
                .post_json_with_headers(
                    &url,
                    &cohere_request_body(&owned),
                    &headers,
                    Some(rate_limit_key),
                )
                .await?;
            let vectors =
                parse_cohere_response(&value).map_err(|()| EmbedError::MissingEmbedding)?;
            if vectors.len() != texts.len() {
                return Err(EmbedError::MissingEmbedding);
            }
            (vectors, None)
        }
    };

    Ok(EmbeddingOutcome {
        model: model_ref.model.clone(),
        embeddings: vectors
            .into_iter()
            .enumerate()
            .map(|(index, vector)| Embedding { index, vector })
            .collect(),
        usage: prompt_tokens.map(|tokens| EmbeddingUsage {
            prompt_tokens: tokens,
            total_tokens: tokens,
        }),
    })
}

impl EmbeddingInput {
    fn texts(&self) -> Vec<&str> {
        match self {
            Self::Text(text) => vec![text.as_str()],
            Self::Texts(texts) => texts.iter().map(String::as_str).collect(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bedrock_unknown_embedding_model_fails_before_http() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let credentials = ProviderCredentials::new();
        let err = embed(
            &http,
            &credentials,
            EmbeddingRequest {
                model: "bedrock:anthropic.claude-sonnet-4".into(),
                input: EmbeddingInput::Text("hello".into()),
                dimensions: None,
            },
        )
        .await
        .expect_err("chat model is not an embedding model");
        assert!(matches!(
            err,
            EmbedError::UnsupportedModel {
                provider: ProviderId::Bedrock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn bedrock_rejects_a_model_id_that_changes_the_path() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let mut credentials = ProviderCredentials::new();
        credentials.insert_key(ProviderId::Bedrock, "bedrock-test");
        let err = embed(
            &http,
            &credentials,
            EmbeddingRequest {
                model: "bedrock:amazon.titan-embed-text/v2".into(),
                input: EmbeddingInput::Text("hello".into()),
                dimensions: None,
            },
        )
        .await
        .expect_err("slash is not a model id");
        assert!(matches!(err, EmbedError::InvalidModelId));
    }

    #[tokio::test]
    async fn bedrock_titan_rejects_more_than_32_texts_before_http() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let credentials = ProviderCredentials::new();
        let err = embed(
            &http,
            &credentials,
            EmbeddingRequest {
                model: "bedrock:amazon.titan-embed-text-v2:0".into(),
                input: EmbeddingInput::Texts(vec!["x".into(); 33]),
                dimensions: None,
            },
        )
        .await
        .expect_err("titan batch is capped");
        assert!(matches!(
            err,
            EmbedError::TooManyInputs { count: 33, max: 32 }
        ));
    }
}
