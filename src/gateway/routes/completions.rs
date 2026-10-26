//! Chat completions and model listing routes.

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::gateway::GatewayState;
use crate::gateway::auth::Auth;
use crate::gateway::error::GatewayError;
use crate::gateway::model_catalog;
use crate::gateway::proxy::{self, GatewayCompletionBody};

pub async fn chat_completions(
    State(state): State<Arc<GatewayState>>,
    Auth(auth): Auth,
    Json(body): Json<GatewayCompletionBody>,
) -> Result<Response, GatewayError> {
    let stream = body.completion.stream.unwrap_or(false);
    if stream {
        let byte_stream = proxy::proxy_completion_stream(&state, auth, body).await?;
        let body = Body::from_stream(byte_stream);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(body)
            .unwrap())
    } else {
        let (val, _request_id) = proxy::proxy_completion(&state, &auth, body).await?;
        Ok(Json(val).into_response())
    }
}

pub async fn list_models(
    State(state): State<Arc<GatewayState>>,
    Auth(auth): Auth,
) -> Result<impl IntoResponse, GatewayError> {
    let master = state.config.master_key_exposed().to_string();
    let proxies = state
        .db
        .run_blocking(move |db| crate::gateway::proxy_provider::enabled_list_targets(db, &master))
        .await?;
    let data = model_catalog::list_gateway_models(
        &state.http,
        &state.proxy_http,
        &state.credentials,
        &auth,
        Some(&state.catalog),
        &proxies,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "object": "list",
        "data": data,
        "refreshed_at": state.catalog.refreshed_at_ms(),
    })))
}
