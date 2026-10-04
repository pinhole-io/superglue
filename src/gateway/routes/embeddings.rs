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

use crate::embeddings::{EmbeddingInput, supports_embeddings};
use crate::gateway::GatewayState;
use crate::gateway::auth::{Auth, resolve_user_id};
use crate::gateway::error::GatewayError;
use crate::gateway::proxy::{preflight_async, record_usage_async};
use crate::http::join_base_url;
use crate::providers::{parse_model_ref, rate_limit_key_for};

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
    let model_ref = parse_model_ref(&body.model);
    if !supports_embeddings(model_ref.provider) {
        return Err(GatewayError::bad_request(format!(
            "embeddings are not supported for {}",
            model_ref.provider
        )));
    }

    let qualified = format!("{}:{}", model_ref.provider.as_str(), model_ref.model);
    let user_id = resolve_user_id(&auth, body.user.as_deref())?;
    preflight_async(&state.db, &auth, &user_id, &qualified).await?;

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
