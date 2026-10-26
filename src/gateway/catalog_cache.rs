//! Cached upstream model lists for the gateway process.
//!
//! The cache fills when the process starts, then again every hour. A list
//! request reads the cache. One refresh runs at a time. A failed refresh keeps
//! the previous list for that provider.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::Mutex;
use tracing::warn;

use crate::gateway::error::GatewayResult;
use crate::gateway::model_catalog::{fetch_provider_models, fetch_proxy_models};
use crate::gateway::proxy_provider::ProxyListTarget;
use crate::http::HttpClient;
use crate::providers::{ProviderCredentials, ProviderId};

/// How often the gateway asks upstream providers for model lists.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Process-local model lists, keyed by provider.
pub struct ModelCatalogCache {
    slots: RwLock<HashMap<ProviderId, Arc<Vec<Value>>>>,
    proxy_slots: RwLock<HashMap<String, Arc<Vec<Value>>>>,
    refreshed_at: RwLock<Option<SystemTime>>,
    /// Serializes refresh and a cold fetch so they do not both call upstream.
    gate: Mutex<()>,
}

impl ModelCatalogCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: RwLock::new(HashMap::new()),
            proxy_slots: RwLock::new(HashMap::new()),
            refreshed_at: RwLock::new(None),
            gate: Mutex::new(()),
        }
    }

    /// Unix milliseconds of the last refresh that stored at least one provider.
    #[must_use]
    pub fn refreshed_at_ms(&self) -> Option<u64> {
        let at = (*self.refreshed_at.read().unwrap_or_else(|e| e.into_inner()))?;
        let millis = at.duration_since(UNIX_EPOCH).ok()?.as_millis();
        u64::try_from(millis).ok()
    }

    /// Models for one provider. A warm cache does not call upstream.
    pub async fn provider_models(
        &self,
        http: &HttpClient,
        credentials: &ProviderCredentials,
        provider: ProviderId,
    ) -> GatewayResult<Vec<Value>> {
        if let Some(models) = self.cached(provider) {
            return Ok(models);
        }
        let _guard = self.gate.lock().await;
        if let Some(models) = self.cached(provider) {
            return Ok(models);
        }
        let models = fetch_provider_models(http, credentials, provider).await?;
        self.store(provider, models.clone());
        Ok(models)
    }

    /// Fetch every configured provider. Keep the previous list when a fetch fails.
    pub async fn refresh_all(&self, http: &HttpClient, credentials: &ProviderCredentials) {
        let _guard = self.gate.lock().await;
        let mut configured = 0_u32;
        let mut stored = 0_u32;
        for provider in ProviderId::ALL {
            if !credentials.has_key(provider) {
                continue;
            }
            configured += 1;
            match fetch_provider_models(http, credentials, provider).await {
                Ok(models) => {
                    self.store(provider, models);
                    stored += 1;
                }
                Err(err) => warn!(
                    provider = %provider,
                    error = %err,
                    "failed to refresh the model catalog"
                ),
            }
        }
        if configured == 0 || stored > 0 {
            *self.refreshed_at.write().unwrap_or_else(|e| e.into_inner()) = Some(SystemTime::now());
        }
    }

    /// Models for one proxy slug. A warm cache does not call upstream.
    pub async fn proxy_models(
        &self,
        http: &HttpClient,
        target: &ProxyListTarget,
    ) -> GatewayResult<Vec<Value>> {
        if let Some(models) = self.cached_proxy(&target.slug) {
            return Ok(models);
        }
        let _guard = self.gate.lock().await;
        if let Some(models) = self.cached_proxy(&target.slug) {
            return Ok(models);
        }
        let models = fetch_proxy_models(http, target).await?;
        self.store_proxy(&target.slug, models.clone());
        Ok(models)
    }

    /// Fetch every enabled proxy. Keep the previous list when a fetch fails.
    pub async fn refresh_proxies(&self, http: &HttpClient, targets: &[ProxyListTarget]) {
        if targets.is_empty() {
            return;
        }
        let _guard = self.gate.lock().await;
        let mut stored = 0_u32;
        for target in targets {
            match fetch_proxy_models(http, target).await {
                Ok(models) => {
                    self.store_proxy(&target.slug, models);
                    stored += 1;
                }
                Err(err) => warn!(
                    slug = %target.slug,
                    error = %err,
                    "failed to refresh the proxy model catalog"
                ),
            }
        }
        if stored > 0 {
            *self.refreshed_at.write().unwrap_or_else(|e| e.into_inner()) = Some(SystemTime::now());
        }
    }

    /// Drop one proxy list so the next read fetches again.
    pub fn invalidate_proxy(&self, slug: &str) {
        self.proxy_slots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(slug);
    }

    fn cached(&self, provider: ProviderId) -> Option<Vec<Value>> {
        self.slots
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&provider)
            .map(|models| models.as_ref().clone())
    }

    fn store(&self, provider: ProviderId, models: Vec<Value>) {
        self.slots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(provider, Arc::new(models));
    }

    fn cached_proxy(&self, slug: &str) -> Option<Vec<Value>> {
        self.proxy_slots
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(slug)
            .map(|models| models.as_ref().clone())
    }

    fn store_proxy(&self, slug: &str, models: Vec<Value>) {
        self.proxy_slots
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(slug.to_string(), Arc::new(models));
    }
}

impl Default for ModelCatalogCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn failed_refresh_keeps_the_previous_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{ "id": "gpt-4o-mini", "object": "model" }]
            })))
            .mount(&server)
            .await;

        let http = HttpClient::new(crate::http::ClientConfig::default()).unwrap();
        let mut credentials = ProviderCredentials::new();
        credentials.insert_key(ProviderId::OpenAi, "sk-test");
        credentials.insert_base_url(ProviderId::OpenAi, server.uri());
        let cache = ModelCatalogCache::new();

        cache.refresh_all(&http, &credentials).await;
        let first = cache
            .provider_models(&http, &credentials, ProviderId::OpenAi)
            .await
            .unwrap();
        assert_eq!(first[0]["id"], "openai:gpt-4o-mini");
        let stamped = cache.refreshed_at_ms();
        assert!(stamped.is_some());

        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        cache.refresh_all(&http, &credentials).await;
        let second = cache
            .provider_models(&http, &credentials, ProviderId::OpenAi)
            .await
            .unwrap();
        assert_eq!(second[0]["id"], "openai:gpt-4o-mini");
        assert_eq!(cache.refreshed_at_ms(), stamped);
    }
}
