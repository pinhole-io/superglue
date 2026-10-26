//! Per-provider API keys and base URL overrides.

use std::collections::HashMap;

use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

use super::provider_id::ProviderId;

#[derive(Debug, Error)]
pub enum CredentialsError {
    #[error("missing API key for provider {0}")]
    MissingKey(ProviderId),
}

#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    keys: HashMap<ProviderId, SecretString>,
    base_urls: HashMap<ProviderId, String>,
}

impl ProviderCredentials {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_key(&mut self, provider: ProviderId, key: impl Into<String>) {
        self.keys.insert(provider, SecretString::from(key.into()));
    }

    pub fn insert_base_url(&mut self, provider: ProviderId, url: impl Into<String>) {
        self.base_urls.insert(provider, url.into());
    }

    pub fn remove_key(&mut self, provider: ProviderId) {
        self.keys.remove(&provider);
    }

    pub fn remove_base_url(&mut self, provider: ProviderId) {
        self.base_urls.remove(&provider);
    }

    /// Load keys from each provider's `env_var_for_key`, plus known aliases.
    #[must_use]
    pub fn from_env() -> Self {
        let mut creds = Self::new();
        for provider in ProviderId::ALL {
            if let Ok(key) = std::env::var(provider.env_var_for_key())
                && !key.is_empty()
            {
                creds.insert_key(provider, key);
            }
        }
        // Some env files use OPENROUTER_GATEWAY_KEY instead of OPENROUTER_API_KEY.
        if !creds.has_key(ProviderId::OpenRouter)
            && let Ok(key) = std::env::var("OPENROUTER_GATEWAY_KEY")
            && !key.is_empty()
        {
            creds.insert_key(ProviderId::OpenRouter, key);
        }
        if let Ok(url) = std::env::var("OPENAI_BASE_URL")
            && !url.is_empty()
        {
            creds.insert_base_url(ProviderId::OpenAi, url);
        }
        if let Ok(url) = std::env::var("TYPESAFE_BASE_URL")
            && !url.is_empty()
        {
            creds.insert_base_url(ProviderId::TypeSafe, url);
        }
        apply_bedrock_env(&mut creds);
        creds
    }

    /// Merge a legacy single key as OpenAI when no OpenAI key is set.
    pub fn with_legacy_openai_key(&mut self, api_key: &str, base_url: Option<&str>) {
        if !api_key.is_empty() && !self.keys.contains_key(&ProviderId::OpenAi) {
            self.insert_key(ProviderId::OpenAi, api_key);
        }
        if let Some(url) = base_url
            && !url.is_empty()
            && !self.base_urls.contains_key(&ProviderId::OpenAi)
        {
            self.insert_base_url(ProviderId::OpenAi, url);
        }
    }

    pub fn key_for(&self, provider: ProviderId) -> Result<SecretString, CredentialsError> {
        self.keys
            .get(&provider)
            .cloned()
            .ok_or(CredentialsError::MissingKey(provider))
    }

    #[must_use]
    pub fn has_key(&self, provider: ProviderId) -> bool {
        self.keys.contains_key(&provider)
    }

    #[must_use]
    pub fn base_url_for(&self, provider: ProviderId) -> String {
        self.base_urls
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| provider.default_base_url().to_string())
    }
}

fn apply_bedrock_env(creds: &mut ProviderCredentials) {
    if !creds.has_key(ProviderId::Bedrock)
        && let Ok(key) = std::env::var("BEDROCK_API_KEY")
        && !key.is_empty()
    {
        creds.insert_key(ProviderId::Bedrock, key);
    }
    let explicit = std::env::var("BEDROCK_BASE_URL").ok();
    let region = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .ok();
    let url = bedrock_openai_base_url(explicit.as_deref(), region.as_deref());
    if url != ProviderId::Bedrock.default_base_url() {
        creds.insert_base_url(ProviderId::Bedrock, url);
    }
}

/// OpenAI-compatible Bedrock base URL, without a `/v1` suffix.
///
/// `explicit` is `BEDROCK_BASE_URL`. `region` is `AWS_REGION` or
/// `AWS_DEFAULT_REGION`. A region must look like `us-east-1` or
/// `us-gov-west-1`, so it cannot change the URL host.
pub(crate) fn bedrock_openai_base_url(explicit: Option<&str>, region: Option<&str>) -> String {
    if let Some(url) = explicit.map(str::trim).filter(|url| !url.is_empty()) {
        return url.to_string();
    }
    if let Some(region) = region.map(str::trim).filter(|region| is_aws_region(region)) {
        return format!("https://bedrock-runtime.{region}.amazonaws.com/openai");
    }
    ProviderId::Bedrock.default_base_url().to_string()
}

/// `us-east-1`, `us-gov-west-1`, and the same shape. Dots and slashes are rejected.
fn is_aws_region(region: &str) -> bool {
    if region.is_empty() || region.len() > 32 {
        return false;
    }
    let mut parts = region.split('-');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.len() < 2 || !is_region_letters(first) {
        return false;
    }
    let mut saw_name = false;
    let mut saw_number = false;
    for part in parts {
        if part.is_empty() {
            return false;
        }
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            if saw_number || !saw_name {
                return false;
            }
            saw_number = true;
            continue;
        }
        if saw_number || !is_region_letters(part) {
            return false;
        }
        saw_name = true;
    }
    saw_name && saw_number
}

fn is_region_letters(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_lowercase())
}

/// Stable bucket id derived from key material (never log the raw secret).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiKeyId(pub u64);

#[must_use]
pub fn api_key_id(key: &SecretString) -> ApiKeyId {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.expose_secret().hash(&mut hasher);
    ApiKeyId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bedrock_base_url_uses_region_or_explicit_override() {
        let default_url = ProviderId::Bedrock.default_base_url();
        assert_eq!(bedrock_openai_base_url(None, None), default_url);
        assert_eq!(
            bedrock_openai_base_url(None, Some("us-east-1")),
            default_url
        );
        assert_eq!(
            bedrock_openai_base_url(None, Some("us-west-2")),
            "https://bedrock-runtime.us-west-2.amazonaws.com/openai"
        );
        assert_eq!(
            bedrock_openai_base_url(None, Some("us-gov-west-1")),
            "https://bedrock-runtime.us-gov-west-1.amazonaws.com/openai"
        );
        assert_eq!(
            bedrock_openai_base_url(Some("http://127.0.0.1:9/openai"), Some("eu-west-1")),
            "http://127.0.0.1:9/openai"
        );
        assert_eq!(bedrock_openai_base_url(None, Some("../evil")), default_url);
        assert_eq!(
            bedrock_openai_base_url(None, Some("us-east-1.evil.com")),
            default_url
        );
        assert_eq!(
            bedrock_openai_base_url(None, Some("us-east-1/openai")),
            default_url
        );
        assert_eq!(bedrock_openai_base_url(None, Some("")), default_url);
        assert_eq!(
            bedrock_openai_base_url(Some("  "), Some("nope")),
            default_url
        );
    }
}
