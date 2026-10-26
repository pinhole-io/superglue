//! OpenAI-compatible embeddings proxy.
//!
//! Public path: `POST /v1/embeddings`.
//! Do not log input text or returned vectors.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::embeddings::{
    EmbeddingInput, EmbeddingOutcome, EmbeddingRequest, embed, supports_embeddings,
};
use crate::gateway::GatewayState;
use crate::gateway::auth::{Auth, resolve_user_id};
use crate::gateway::error::GatewayError;
use crate::gateway::proxy::{preflight_async, record_usage_async};
use crate::gateway::proxy_provider::{self, ProxyTarget};
use crate::http::join_base_url;
use crate::providers::{ProviderId, parse_model_ref, rate_limit_key_for};
use crate::usage::{UsageBreakdown, usage_from_breakdown};

#[derive(Debug, Deserialize)]
pub struct GatewayEmbeddingBody {
    model: String,
    input: EmbeddingInput,
    #[serde(default)]
    dimensions: Option<usize>,
    #[serde(default)]
    user: Option<String>,
}

/// Proxy an embedding request to an OpenAI-compatible provider.
pub async fn create_embedding(
    State(state): State<Arc<GatewayState>>,
    Auth(auth): Auth,
    Json(body): Json<GatewayEmbeddingBody>,
) -> Result<Json<Value>, GatewayError> {
    if proxy_provider::is_proxy_model(&body.model) {
        return proxy_embedding(&state, &auth, body).await;
    }

    let model_ref = parse_model_ref(&body.model);
    if !supports_embeddings(model_ref.provider) {
        return Err(GatewayError::bad_request(format!(
            "embeddings are not supported for {}",
            model_ref.provider
        )));
    }

    let qualified = model_ref.qualified();
    let user_id = resolve_user_id(&auth, body.user.as_deref())?;
    preflight_async(&state.db, &auth, &user_id, &qualified).await?;

    if model_ref.provider == ProviderId::Bedrock {
        let outcome = embed(
            &state.http,
            &state.credentials,
            EmbeddingRequest {
                model: body.model,
                input: body.input,
                dimensions: body.dimensions,
            },
        )
        .await
        .map_err(embed_error)?;
        if let Some(usage) = outcome.usage {
            let prompt = u32::try_from(usage.prompt_tokens).unwrap_or(u32::MAX);
            let total = u32::try_from(usage.total_tokens).unwrap_or(u32::MAX);
            record_usage_async(
                &state.db,
                auth.key_id.as_deref(),
                &user_id,
                &qualified,
                &usage_from_breakdown(UsageBreakdown {
                    prompt_tokens: prompt,
                    completion_tokens: 0,
                    total_tokens: Some(total),
                    cached_tokens: None,
                    reasoning_tokens: None,
                }),
                0.0,
                &Uuid::new_v4().to_string(),
            )
            .await?;
        }
        return Ok(Json(openai_embedding_json(&outcome)));
    }

    let key = state
        .credentials
        .key_for(model_ref.provider)
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    let rate_limit_key = rate_limit_key_for(&model_ref, &state.credentials)
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    let base_url = state.credentials.base_url_for(model_ref.provider);
    let url = join_base_url(&base_url, "/v1/embeddings");
    let auth_header = format!("Bearer {}", key.expose_secret());
    let mut upstream_body = json!({
        "model": model_ref.model,
        "input": body.input,
    });
    if let Some(dimensions) = body.dimensions {
        upstream_body["dimensions"] = json!(dimensions);
    }

    let value = state
        .http
        .post_json_with_headers(
            &url,
            &upstream_body,
            &[("Authorization", auth_header.as_str())],
            Some(rate_limit_key),
        )
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))?;

    if let Some(usage_value) = value.get("usage")
        && let Ok(usage) = serde_json::from_value::<crate::openai::Usage>(usage_value.clone())
    {
        let usage = crate::usage::usage_from_compat(&usage);
        record_usage_async(
            &state.db,
            auth.key_id.as_deref(),
            &user_id,
            &qualified,
            &usage,
            0.0,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }

    Ok(Json(value))
}

async fn proxy_embedding(
    state: &GatewayState,
    auth: &crate::gateway::auth::AuthContext,
    body: GatewayEmbeddingBody,
) -> Result<Json<Value>, GatewayError> {
    let qualified = body.model.clone();
    let user_id = resolve_user_id(auth, body.user.as_deref())?;
    preflight_async(&state.db, auth, &user_id, &qualified).await?;
    let model = body.model.clone();
    let master = state.config.master_key_exposed().to_string();
    let target = state
        .db
        .run_blocking(move |db| proxy_provider::load_proxy_target(db, &master, &model))
        .await?;
    let upstream = embedding_upstream_body(&body, &target);
    let url = proxy_provider::join_proxy_url(&target.base_url, "/v1/embeddings");
    let bearer = proxy_provider::authorization_value(&target);
    let mut headers = Vec::new();
    if let Some(ref value) = bearer {
        headers.push(("Authorization", value.as_str()));
    }
    let value = state
        .proxy_http
        .post_json_with_headers(&url, &upstream, &headers, None)
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    if let Some(usage_value) = value.get("usage")
        && let Ok(usage) = serde_json::from_value::<crate::openai::Usage>(usage_value.clone())
    {
        let mut usage = crate::usage::usage_from_compat(&usage);
        let cost = usage.cost_usd.unwrap_or(0.0);
        usage.cost_usd = Some(cost);
        record_usage_async(
            &state.db,
            auth.key_id.as_deref(),
            &user_id,
            &target.raw,
            &usage,
            cost,
            &Uuid::new_v4().to_string(),
        )
        .await?;
    }
    Ok(Json(value))
}

fn embedding_upstream_body(body: &GatewayEmbeddingBody, target: &ProxyTarget) -> Value {
    let mut upstream = json!({
        "model": target.wire_model,
        "input": body.input,
    });
    if let Some(dimensions) = body.dimensions {
        upstream["dimensions"] = json!(dimensions);
    }
    if let Some(user) = body.user.as_deref() {
        upstream["user"] = json!(user);
    }
    upstream
}

fn openai_embedding_json(outcome: &EmbeddingOutcome) -> Value {
    json!({
        "object": "list",
        "data": outcome.embeddings.iter().map(|row| json!({
            "object": "embedding",
            "index": row.index,
            "embedding": row.vector,
        })).collect::<Vec<_>>(),
        "model": outcome.model,
        "usage": outcome.usage.map(|usage| json!({
            "prompt_tokens": usage.prompt_tokens,
            "total_tokens": usage.total_tokens,
        })),
    })
}

fn embed_error(error: crate::embeddings::EmbedError) -> GatewayError {
    use crate::embeddings::EmbedError;
    let message = error.to_string();
    match error {
        EmbedError::UnsupportedProvider(_)
        | EmbedError::UnsupportedModel { .. }
        | EmbedError::EmptyInput
        | EmbedError::TooManyInputs { .. }
        | EmbedError::InvalidModelId => GatewayError::bad_request(message),
        EmbedError::Http(_) | EmbedError::Credentials(_) | EmbedError::MissingEmbedding => {
            GatewayError::upstream(message)
        }
        EmbedError::Serde(_) => GatewayError::Internal(message),
    }
}
