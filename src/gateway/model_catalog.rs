//! Upstream model catalog for unrestricted keys and provider wildcards.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tracing::warn;

use crate::gateway::auth::AuthContext;
use crate::gateway::error::{GatewayError, GatewayResult};
use crate::gateway::model_access::is_unrestricted;
use crate::gateway::proxy_provider::{self, ProxyListTarget};
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

/// List builtin and proxy models for a gateway caller.
pub async fn list_gateway_models(
    http: &HttpClient,
    proxy_http: &HttpClient,
    credentials: &ProviderCredentials,
    auth: &AuthContext,
    cache: Option<&super::catalog_cache::ModelCatalogCache>,
    proxies: &[ProxyListTarget],
) -> GatewayResult<Vec<Value>> {
    assemble_model_list(http, proxy_http, credentials, auth, cache, proxies).await
}

async fn assemble_model_list(
    http: &HttpClient,
    proxy_http: &HttpClient,
    credentials: &ProviderCredentials,
    auth: &AuthContext,
    cache: Option<&super::catalog_cache::ModelCatalogCache>,
    proxies: &[ProxyListTarget],
) -> GatewayResult<Vec<Value>> {
    if is_unrestricted(&auth.allowed_models) {
        return list_all_provider_models(http, proxy_http, credentials, cache, proxies).await;
    }

    let patterns = auth.allowed_models.as_deref().unwrap_or(&[]);
    if patterns.iter().any(|p| p == "*") {
        return list_all_provider_models(http, proxy_http, credentials, cache, proxies).await;
    }

    let mut result = Vec::new();
    let mut fetch_providers = HashSet::new();
    let mut fetch_proxy_slugs = HashSet::new();
    let mut literals = Vec::new();
    let mut enrich_providers = HashSet::new();
    let mut enrich_proxy_slugs = HashSet::new();

    for pattern in patterns {
        if let Some(prefix) = pattern.strip_suffix(":*") {
            if let Ok(provider) = ProviderId::from_str(prefix) {
                fetch_providers.insert(provider);
                continue;
            }
            let slug = prefix.to_ascii_lowercase();
            if proxies.iter().any(|proxy| proxy.slug == slug) {
                fetch_proxy_slugs.insert(slug);
                continue;
            }
        }
        if let Some(provider) = provider_for_model_id(pattern) {
            enrich_providers.insert(provider);
        } else if let Some(slug) = proxy_slug_for_literal(pattern, proxies) {
            enrich_proxy_slugs.insert(slug);
        }
        literals.push(pattern.to_string());
    }

    let mut windows = HashMap::new();
    for provider in &fetch_providers {
        match load_provider_models(http, credentials, *provider, cache).await {
            Ok(models) if !models.is_empty() => {
                record_windows(&models, &mut windows);
                result.extend(models);
            }
            _ => result.push(model_entry(&format!("{}:*", provider.as_str()))),
        }
    }

    for slug in &fetch_proxy_slugs {
        let Some(target) = proxies.iter().find(|proxy| proxy.slug == *slug) else {
            continue;
        };
        match load_proxy_models(proxy_http, target, cache).await {
            Ok(models) if !models.is_empty() => {
                record_windows(&models, &mut windows);
                result.extend(models);
            }
            _ => result.push(model_entry(&format!("{slug}:*"))),
        }
    }

    for slug in enrich_proxy_slugs.difference(&fetch_proxy_slugs) {
        let Some(target) = proxies.iter().find(|proxy| proxy.slug == *slug) else {
            continue;
        };
        match load_proxy_models(proxy_http, target, cache).await {
            Ok(models) => record_windows(&models, &mut windows),
            Err(err) => warn!(
                slug = %slug,
                error = %err,
                "failed to enrich literal proxy model context windows"
            ),
        }
    }

    for provider in enrich_providers.difference(&fetch_providers) {
        if !credentials.has_key(*provider) {
            continue;
        }
        match load_provider_models(http, credentials, *provider, cache).await {
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
    proxy_http: &HttpClient,
    credentials: &ProviderCredentials,
    cache: Option<&super::catalog_cache::ModelCatalogCache>,
    proxies: &[ProxyListTarget],
) -> GatewayResult<Vec<Value>> {
    let mut result = Vec::new();
    for provider in ProviderId::ALL {
        if !credentials.has_key(provider) {
            continue;
        }
        match load_provider_models(http, credentials, provider, cache).await {
            Ok(mut models) => result.append(&mut models),
            Err(e) => warn!(provider = %provider, error = %e, "failed to list upstream models"),
        }
    }
    for target in proxies {
        match load_proxy_models(proxy_http, target, cache).await {
            Ok(mut models) => result.append(&mut models),
            Err(err) => warn!(
                slug = %target.slug,
                error = %err,
                "failed to list proxy models"
            ),
        }
    }
    dedupe_sort_models(&mut result);
    Ok(result)
}

fn proxy_slug_for_literal(pattern: &str, proxies: &[ProxyListTarget]) -> Option<String> {
    let (prefix, _) = pattern.split_once(':')?;
    let slug = prefix.to_ascii_lowercase();
    if ProviderId::from_str(&slug).is_ok() {
        return None;
    }
    proxies
        .iter()
        .find(|proxy| proxy.slug == slug)
        .map(|proxy| proxy.slug.clone())
}

async fn load_proxy_models(
    http: &HttpClient,
    target: &ProxyListTarget,
    cache: Option<&super::catalog_cache::ModelCatalogCache>,
) -> GatewayResult<Vec<Value>> {
    if let Some(cache) = cache {
        return cache.proxy_models(http, target).await;
    }
    fetch_proxy_models(http, target).await
}

pub(crate) async fn fetch_proxy_models(
    http: &HttpClient,
    target: &ProxyListTarget,
) -> GatewayResult<Vec<Value>> {
    let url = proxy_provider::join_proxy_url(&target.base_url, "/v1/models");
    let bearer = target
        .api_key
        .as_ref()
        .map(|key| format!("Bearer {}", key.expose_secret()));
    let headers: Vec<(&str, &str)> = match bearer.as_deref() {
        Some(value) => vec![("Authorization", value)],
        None => Vec::new(),
    };
    let header_ref = if headers.is_empty() {
        None
    } else {
        Some(headers.as_slice())
    };
    let bytes = http
        .get_with_headers(&url, header_ref)
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
                &format!("{}:{id}", target.slug),
                upstream_context_length(entry),
            ))
        })
        .collect())
}

async fn load_provider_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    provider: ProviderId,
    cache: Option<&super::catalog_cache::ModelCatalogCache>,
) -> GatewayResult<Vec<Value>> {
    if let Some(cache) = cache {
        return cache.provider_models(http, credentials, provider).await;
    }
    fetch_provider_models(http, credentials, provider).await
}

pub(crate) async fn fetch_provider_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    provider: ProviderId,
) -> GatewayResult<Vec<Value>> {
    if provider == ProviderId::TypeSafe {
        return fetch_typesafe_models(http, credentials).await;
    }
    if provider == ProviderId::Bedrock {
        return fetch_bedrock_models(http, credentials).await;
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

async fn fetch_bedrock_models(
    http: &HttpClient,
    credentials: &ProviderCredentials,
) -> GatewayResult<Vec<Value>> {
    use crate::providers::bedrock::{
        MAX_PROFILE_PAGES, bedrock_hosts, catalog_models, foundation_models_url,
        inference_profiles_url, page_token,
    };

    let key = credentials
        .key_for(ProviderId::Bedrock)
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let hosts = bedrock_hosts(&credentials.base_url_for(ProviderId::Bedrock));
    let bearer = format!("Bearer {}", key.expose_secret());
    let headers = [("Authorization", bearer.as_str())];

    let foundation_bytes = http
        .get_with_headers(&foundation_models_url(&hosts.control), Some(&headers))
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    let foundation: Value = serde_json::from_slice(&foundation_bytes)
        .map_err(|error| GatewayError::Internal(format!("invalid models JSON: {error}")))?;

    let mut profile_pages = Vec::new();
    let mut token = None;
    for _ in 0..MAX_PROFILE_PAGES {
        let url = inference_profiles_url(&hosts.control, token.as_deref())
            .map_err(|_| GatewayError::upstream("invalid inference profile page token"))?;
        let bytes = http
            .get_with_headers(&url, Some(&headers))
            .await
            .map_err(|error| GatewayError::upstream(error.to_string()))?;
        let page: Value = serde_json::from_slice(&bytes)
            .map_err(|error| GatewayError::Internal(format!("invalid models JSON: {error}")))?;
        token = page_token(&page)
            .map_err(|_| GatewayError::upstream("invalid inference profile page token"))?;
        let done = token.is_none();
        profile_pages.push(page);
        if done {
            break;
        }
    }

    let models = catalog_models(&foundation, &profile_pages)
        .map_err(|_| GatewayError::upstream("invalid bedrock model list"))?;
    Ok(models
        .into_iter()
        .map(|model| {
            let id = if model.embedding {
                format!("bedrock:embedding:{}", model.id)
            } else {
                format!("bedrock:{}", model.id)
            };
            model_entry(&id)
        })
        .collect())
}

/// Probe upstream catalog for admin provider status (returns model count).
pub async fn fetch_provider_models_for_status(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    provider: ProviderId,
    cache: &super::catalog_cache::ModelCatalogCache,
) -> GatewayResult<u32> {
    let models = cache.provider_models(http, credentials, provider).await?;
    Ok(u32::try_from(models.len()).unwrap_or(u32::MAX))
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
        let models = rt
            .block_on(list_gateway_models(&http, &http, &creds, &auth, None, &[]))
            .unwrap();
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
    async fn bedrock_catalog_lists_foundation_models_and_profiles() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/foundation-models"))
            .and(query_param("byInferenceType", "ON_DEMAND"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "modelSummaries": [
                    {
                        "modelId": "amazon.titan-embed-text-v2:0",
                        "outputModalities": ["EMBEDDING"]
                    },
                    {
                        "modelId": "anthropic.claude-3-haiku-20240307-v1:0",
                        "outputModalities": ["TEXT"]
                    }
                ]
            })))
            .mount(&server)
            .await;
        // First registered mock wins when both match. The page-2 mock must be first.
        Mock::given(method("GET"))
            .and(path("/inference-profiles"))
            .and(query_param("nextToken", "page-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "inferenceProfileSummaries": [{
                    "inferenceProfileId": "us.amazon.titan-embed-text-v2:0",
                    "models": [{
                        "modelArn": "arn:aws:bedrock:us-east-1::foundation-model/amazon.titan-embed-text-v2:0"
                    }]
                }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/inference-profiles"))
            .and(query_param("maxResults", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "inferenceProfileSummaries": [{
                    "inferenceProfileId": "us.anthropic.claude-3-haiku-20240307-v1:0",
                    "models": [{
                        "modelArn": "arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-3-haiku-20240307-v1:0"
                    }]
                }],
                "nextToken": "page-2"
            })))
            .mount(&server)
            .await;

        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::Bedrock, "bedrock-test");
        creds.insert_base_url(ProviderId::Bedrock, format!("{}/openai", server.uri()));
        let models = fetch_provider_models(&http, &creds, ProviderId::Bedrock)
            .await
            .expect("bedrock catalog");
        let ids: Vec<_> = models
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "bedrock:embedding:amazon.titan-embed-text-v2:0",
                "bedrock:anthropic.claude-3-haiku-20240307-v1:0",
                "bedrock:us.anthropic.claude-3-haiku-20240307-v1:0",
                "bedrock:embedding:us.amazon.titan-embed-text-v2:0",
            ]
        );
        let embedding = models
            .iter()
            .find(|model| model["id"] == "bedrock:embedding:amazon.titan-embed-text-v2:0")
            .expect("embedding model");
        assert_eq!(embedding["capabilities"], json!(["embedding"]));
    }

    #[tokio::test]
    async fn unrestricted_without_credentials_is_empty() {
        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let creds = ProviderCredentials::new();
        let models = list_gateway_models(&http, &http, &creds, &unrestricted_auth(), None, &[])
            .await
            .unwrap();
        assert!(models.is_empty());
    }
}
