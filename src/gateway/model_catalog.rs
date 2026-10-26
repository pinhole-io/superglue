//! Upstream model catalog for unrestricted keys and provider wildcards.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tracing::warn;

use crate::gateway::auth::AuthContext;
use crate::gateway::error::{GatewayError, GatewayResult};
use crate::gateway::model_access::is_unrestricted;
use crate::http::{HttpClient, join_base_url};
use crate::providers::{ProviderCredentials, ProviderId};

fn model_entry(id: &str) -> Value {
    model_entry_with_context(id, None)
}

fn model_entry_with_context(id: &str, context_length: Option<u32>) -> Value {
    let capabilities = match id
        .split_once(':')
        .and_then(|(_, model)| model.strip_prefix("embedding:"))
    {
        Some(model) if !model.is_empty() && !model.contains('*') => vec!["embedding"],
        _ => Vec::new(),
    };
    let mut entry = json!({
        "id": id,
        "object": "model",
        "owned_by": "superglue-gateway",
        "capabilities": capabilities,
    });
    if let Some(tokens) = context_length.filter(|n| *n > 0) {
        entry["context_length"] = json!(tokens);
    }
    entry
}

/// Positive context window from an upstream model object.
fn upstream_context_length(entry: &Value) -> Option<u32> {
    let value = entry.get("context_length")?;
    let n = value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))?;
    u32::try_from(n).ok().filter(|n| *n > 0)
}

/// Window for a literal allowlisted id from a provider catalog map.
fn context_for_literal(id: &str, windows: &HashMap<String, u32>) -> Option<u32> {
    if let Some(tokens) = windows.get(id).copied().filter(|n| *n > 0) {
        return Some(tokens);
    }
    // `openai:embedding:text-embedding-3-small` matches upstream `openai:text-embedding-3-small`.
    let (provider, rest) = id.split_once(':')?;
    let model = rest.strip_prefix("embedding:")?;
    if model.is_empty() {
        return None;
    }
    windows
        .get(&format!("{provider}:{model}"))
        .copied()
        .filter(|n| *n > 0)
}

fn record_windows(models: &[Value], windows: &mut HashMap<String, u32>) {
    for model in models {
        let Some(id) = model.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(tokens) = model
            .get("context_length")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
            })
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0)
        else {
            continue;
        };
        windows.insert(id.to_string(), tokens);
    }
}

fn provider_for_model_id(id: &str) -> Option<ProviderId> {
    let (prefix, _) = id.split_once(':')?;
    ProviderId::from_str(prefix).ok()
}

fn dedupe_sort_models(models: &mut Vec<Value>) {
    models.sort_by(|a, b| {
        a["id"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["id"].as_str().unwrap_or_default())
    });
    models.dedup_by(|a, b| a["id"] == b["id"]);
}

/// List models visible to the caller, fetching upstream catalogs when unrestricted.
pub async fn list_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    auth: &AuthContext,
) -> GatewayResult<Vec<Value>> {
    if is_unrestricted(&auth.allowed_models) {
        return list_all_provider_models(http, credentials).await;
    }

    let patterns = auth.allowed_models.as_deref().unwrap_or(&[]);
    if patterns.iter().any(|p| p == "*") {
        return list_all_provider_models(http, credentials).await;
    }

    let mut result = Vec::new();
    let mut fetch_providers = HashSet::new();
    let mut literals = Vec::new();
    let mut enrich_providers = HashSet::new();

    for pattern in patterns {
        if let Some(prefix) = pattern.strip_suffix(":*") {
            if let Ok(provider) = ProviderId::from_str(prefix) {
                fetch_providers.insert(provider);
                continue;
            }
        }
        if let Some(provider) = provider_for_model_id(pattern) {
            enrich_providers.insert(provider);
        }
        literals.push(pattern.to_string());
    }

    let mut windows = HashMap::new();
    for provider in &fetch_providers {
        match fetch_provider_models(http, credentials, *provider).await {
            Ok(models) if !models.is_empty() => {
                record_windows(&models, &mut windows);
                result.extend(models);
            }
            _ => result.push(model_entry(&format!("{}:*", provider.as_str()))),
        }
    }

    for provider in enrich_providers.difference(&fetch_providers) {
        if !credentials.has_key(*provider) {
            continue;
        }
        match fetch_provider_models(http, credentials, *provider).await {
            Ok(models) => record_windows(&models, &mut windows),
            Err(err) => warn!(
                provider = %provider,
                error = %err,
                "failed to enrich literal model context windows"
            ),
        }
    }

    for id in literals {
        let context_length = context_for_literal(&id, &windows);
        result.push(model_entry_with_context(&id, context_length));
    }

    dedupe_sort_models(&mut result);
    Ok(result)
}

async fn list_all_provider_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
) -> GatewayResult<Vec<Value>> {
    let mut result = Vec::new();
    for provider in ProviderId::ALL {
        if !credentials.has_key(provider) {
            continue;
        }
        match fetch_provider_models(http, credentials, provider).await {
            Ok(mut models) => result.append(&mut models),
            Err(e) => warn!(provider = %provider, error = %e, "failed to list upstream models"),
        }
    }
    dedupe_sort_models(&mut result);
    Ok(result)
}

async fn fetch_provider_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    provider: ProviderId,
) -> GatewayResult<Vec<Value>> {
    if provider == ProviderId::TypeSafe {
        return fetch_typesafe_models(http, credentials).await;
    }
    if !provider.lists_models() {
        return Ok(Vec::new());
    }

    let key = credentials
        .key_for(provider)
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let base = credentials.base_url_for(provider);
    let url = join_base_url(&base, "/v1/models");
    let bearer = format!("Bearer {}", key.expose_secret());

    let headers: Vec<(&str, &str)> = match provider {
        ProviderId::Anthropic => vec![
            ("x-api-key", key.expose_secret()),
            ("anthropic-version", "2023-06-01"),
        ],
        _ => vec![("Authorization", bearer.as_str())],
    };

    let bytes = http
        .get_with_headers(&url, Some(&headers))
        .await
        .map_err(|e| GatewayError::upstream(e.to_string()))?;
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|e| GatewayError::Internal(format!("invalid models JSON: {e}")))?;

    let models = body["data"]
        .as_array()
        .ok_or_else(|| GatewayError::upstream("upstream models response missing data array"))?;

    Ok(models
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_str()?;
            Some(model_entry_with_context(
                &format!("{}:{id}", provider.as_str()),
                upstream_context_length(entry),
            ))
        })
        .collect())
}

async fn fetch_typesafe_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
) -> GatewayResult<Vec<Value>> {
    match fetch_typesafe_models_inner(http, credentials).await {
        Ok(models) if !models.is_empty() => Ok(models),
        _ => Ok(vec![model_entry("typesafe:jev-latest")]),
    }
}

async fn fetch_typesafe_models_inner(
    http: &HttpClient,
    credentials: &ProviderCredentials,
) -> GatewayResult<Vec<Value>> {
    let key = credentials
        .key_for(ProviderId::TypeSafe)
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let base = credentials.base_url_for(ProviderId::TypeSafe);
    let url = join_base_url(&base, "/v1/models");
    let bearer = format!("Bearer {}", key.expose_secret());
    let bytes = http
        .get_with_headers(&url, Some(&[("Authorization", bearer.as_str())]))
        .await
        .map_err(|e| GatewayError::upstream(e.to_string()))?;
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|e| GatewayError::Internal(format!("invalid models JSON: {e}")))?;

    let from_models = body["models"].as_array().map(|entries| {
        entries
            .iter()
            .filter_map(|entry| {
                let id = entry["name"].as_str().or_else(|| entry["id"].as_str())?;
                Some(model_entry_with_context(
                    &format!("typesafe:{id}"),
                    upstream_context_length(entry),
                ))
            })
            .collect::<Vec<_>>()
    });
    if let Some(models) = from_models.filter(|models| !models.is_empty()) {
        return Ok(models);
    }

    let from_data = body["data"].as_array().map(|entries| {
        entries
            .iter()
            .filter_map(|entry| {
                let id = entry["id"].as_str().or_else(|| entry["name"].as_str())?;
                Some(model_entry_with_context(
                    &format!("typesafe:{id}"),
                    upstream_context_length(entry),
                ))
            })
            .collect::<Vec<_>>()
    });
    Ok(from_data.unwrap_or_default())
}

/// Probe upstream catalog for admin provider status (returns model count).
pub async fn fetch_provider_models_for_status(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    provider: ProviderId,
) -> GatewayResult<u32> {
    let models = fetch_provider_models(http, credentials, provider).await?;
    Ok(models.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::auth::AuthContext;

    fn unrestricted_auth() -> AuthContext {
        AuthContext {
            key_id: None,
            user_id: String::new(),
            is_master: true,
            allowed_models: None,
            max_reasoning_effort: None,
        }
    }

    #[test]
    fn literal_patterns_are_returned_without_fetch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let auth = AuthContext {
            key_id: Some("k1".into()),
            user_id: "u1".into(),
            is_master: false,
            allowed_models: Some(vec![
                "openai:gpt-4o-mini".into(),
                "openai:embedding:text-embedding-3-small".into(),
                "anthropic:claude-3-5-sonnet-20241022".into(),
            ]),
            max_reasoning_effort: None,
        };
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let creds = ProviderCredentials::new();
        let models = rt.block_on(list_models(&http, &creds, &auth)).unwrap();
        let ids: Vec<_> = models.iter().filter_map(|m| m["id"].as_str()).collect();
        assert!(ids.contains(&"openai:gpt-4o-mini"));
        assert!(ids.contains(&"anthropic:claude-3-5-sonnet-20241022"));
        assert!(!ids.contains(&"*"));
        let embedding = models
            .iter()
            .find(|model| model["id"] == "openai:embedding:text-embedding-3-small")
            .unwrap();
        assert_eq!(embedding["capabilities"], json!(["embedding"]));
        let chat = models
            .iter()
            .find(|model| model["id"] == "openai:gpt-4o-mini")
            .unwrap();
        assert_eq!(chat["capabilities"], json!([]));
        assert!(chat.get("context_length").is_none());
        assert_eq!(model_entry("openai:embedding:*")["capabilities"], json!([]));
    }

    #[test]
    fn upstream_context_length_is_copied() {
        let with_window = model_entry_with_context(
            "openrouter:deepseek/deepseek-v4.1-flash",
            upstream_context_length(
                &json!({"id": "deepseek/deepseek-v4.1-flash", "context_length": 1_048_576}),
            ),
        );
        assert_eq!(with_window["context_length"], json!(1_048_576));

        let missing = model_entry_with_context(
            "openai:gpt-4o",
            upstream_context_length(&json!({"id": "gpt-4o"})),
        );
        assert!(missing.get("context_length").is_none());

        let zero = model_entry_with_context(
            "openai:gpt-4o",
            upstream_context_length(&json!({"id": "gpt-4o", "context_length": 0})),
        );
        assert!(zero.get("context_length").is_none());
    }

    #[test]
    fn literal_ids_use_catalog_windows() {
        let mut windows = HashMap::new();
        windows.insert(
            "openrouter:deepseek/deepseek-v4.1-flash".to_string(),
            1_048_576,
        );
        windows.insert("openai:text-embedding-3-small".to_string(), 8_191);
        assert_eq!(
            context_for_literal("openrouter:deepseek/deepseek-v4.1-flash", &windows),
            Some(1_048_576)
        );
        assert_eq!(
            context_for_literal("openai:embedding:text-embedding-3-small", &windows),
            Some(8_191)
        );
        assert_eq!(context_for_literal("openai:gpt-6-luna", &windows), None);
    }

    #[tokio::test]
    async fn bedrock_catalog_status_skips_upstream() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::Bedrock, "bedrock-test");
        creds.insert_base_url(ProviderId::Bedrock, "http://127.0.0.1:1");
        let count = fetch_provider_models_for_status(&http, &creds, ProviderId::Bedrock)
            .await
            .expect("bedrock catalog is empty without a request");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn unrestricted_without_credentials_is_empty() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let creds = ProviderCredentials::new();
        let models = list_models(&http, &creds, &unrestricted_auth())
            .await
            .unwrap();
        assert!(models.is_empty());
    }
}
