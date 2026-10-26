//! Master-key CRUD for dynamic proxy providers.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;

use crate::gateway::GatewayState;
use crate::gateway::auth::Auth;
use crate::gateway::error::GatewayResult;
use crate::gateway::proxy_provider::{self, ProxyProviderChanges, ProxyProviderInput};
use crate::gateway::routes::json_ok;

#[derive(Debug, Deserialize)]
pub struct CreateProxyProviderBody {
    pub slug: String,
    pub label: Option<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProxyProviderBody {
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    /// Absent keeps the stored key. An empty string clears it.
    pub api_key: Option<String>,
}

fn default_enabled() -> bool {
    true
}

pub async fn create_proxy_provider(
    State(state): State<Arc<GatewayState>>,
    Auth(_auth): Auth,
    Json(body): Json<CreateProxyProviderBody>,
) -> GatewayResult<impl axum::response::IntoResponse> {
    let master = state.config.master_key_exposed().to_string();
    let record = state
        .db
        .run_blocking(move |db| {
            proxy_provider::create_proxy_provider(
                db,
                &master,
                ProxyProviderInput {
                    slug: body.slug,
                    label: body.label,
                    base_url: body.base_url,
                    api_key: body.api_key,
                    enabled: body.enabled,
                },
            )
        })
        .await?;
    state.catalog.invalidate_proxy(&record.slug);
    Ok(json_ok(record))
}

pub async fn list_proxy_providers(
    State(state): State<Arc<GatewayState>>,
    Auth(_auth): Auth,
) -> GatewayResult<impl axum::response::IntoResponse> {
    let providers = state
        .db
        .run_blocking(|db| db.list_proxy_providers())
        .await?;
    Ok(json_ok(serde_json::json!({ "providers": providers })))
}

pub async fn update_proxy_provider(
    State(state): State<Arc<GatewayState>>,
    Auth(_auth): Auth,
    Path(slug): Path<String>,
    Json(body): Json<UpdateProxyProviderBody>,
) -> GatewayResult<impl axum::response::IntoResponse> {
    let master = state.config.master_key_exposed().to_string();
    let record = state
        .db
        .run_blocking(move |db| {
            proxy_provider::update_proxy_provider(
                db,
                &master,
                &slug,
                ProxyProviderChanges {
                    label: body.label,
                    base_url: body.base_url,
                    enabled: body.enabled,
                    api_key: body.api_key,
                },
            )
        })
        .await?;
    state.catalog.invalidate_proxy(&record.slug);
    Ok(json_ok(record))
}

pub async fn delete_proxy_provider(
    State(state): State<Arc<GatewayState>>,
    Auth(_auth): Auth,
    Path(slug): Path<String>,
) -> GatewayResult<impl axum::response::IntoResponse> {
    let deleted = state
        .db
        .run_blocking(move |db| proxy_provider::delete_proxy_provider(db, &slug))
        .await?;
    state.catalog.invalidate_proxy(&deleted);
    Ok(json_ok(serde_json::json!({ "deleted": deleted })))
}
