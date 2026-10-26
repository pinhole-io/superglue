//! Dynamic OpenAI-compatible upstreams addressed as `slug:model`.

use std::str::FromStr;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tracing::warn;

use crate::gateway::auth::AuthContext;
use crate::gateway::db::{Database, NewProxyProvider, ProxyProviderRecord, ProxyProviderStored};
use crate::gateway::error::{GatewayError, GatewayResult};
use crate::gateway::proxy_secret::{self, SealedKey};
use crate::http::join_base_url;
use crate::providers::ProviderId;

/// Upstream selected for one proxy model reference.
pub struct ProxyTarget {
    /// Caller model string, used for allowlists and usage.
    pub raw: String,
    /// Model id sent to the upstream API.
    pub wire_model: String,
    pub embedding: bool,
    pub base_url: String,
    pub api_key: Option<SecretString>,
}

/// Enabled proxy used to list upstream models.
#[derive(Clone)]
pub struct ProxyListTarget {
    pub slug: String,
    pub base_url: String,
    pub api_key: Option<SecretString>,
}

/// Fields for `POST /v1/proxy-providers`.
pub struct ProxyProviderInput {
    pub slug: String,
    pub label: Option<String>,
    pub base_url: String,
    pub api_key: Option<String>,
    pub enabled: bool,
}

/// Fields for `PATCH /v1/proxy-providers/{slug}`.
///
/// `api_key: None` keeps the stored key. An empty string clears it.
pub struct ProxyProviderChanges {
    pub label: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub api_key: Option<String>,
}

/// True when the first label is not a builtin provider.
#[must_use]
pub fn is_proxy_model(model: &str) -> bool {
    let Some((prefix, _)) = model.trim().split_once(':') else {
        return false;
    };
    !prefix.is_empty() && ProviderId::from_str(prefix).is_err()
}

/// Load one proxy row for a `slug:model` reference.
pub fn load_proxy_target(
    db: &Database,
    master_key: &str,
    model: &str,
) -> GatewayResult<ProxyTarget> {
    let trimmed = model.trim();
    let Some((prefix, rest)) = trimmed.split_once(':') else {
        return Err(GatewayError::bad_request("proxy model must use slug:model"));
    };
    if rest.is_empty() {
        return Err(GatewayError::bad_request("proxy model id is empty"));
    }
    let slug = prefix.to_ascii_lowercase();
    let Some(row) = db.get_proxy_provider_secret(&slug)? else {
        return Err(GatewayError::bad_request(format!(
            "unknown provider '{slug}'"
        )));
    };
    if !row.enabled {
        return Err(GatewayError::forbidden(format!(
            "proxy provider '{slug}' is disabled"
        )));
    }
    let (embedding, wire_model) = split_wire_model(rest);
    if wire_model.is_empty() {
        return Err(GatewayError::bad_request("proxy model id is empty"));
    }
    let api_key = open_stored_key(&row, master_key)?;
    Ok(ProxyTarget {
        raw: trimmed.to_string(),
        wire_model,
        embedding,
        base_url: row.base_url,
        api_key,
    })
}

/// Enabled proxies whose keys can be opened. A corrupt row is skipped.
pub fn enabled_list_targets(
    db: &Database,
    master_key: &str,
) -> GatewayResult<Vec<ProxyListTarget>> {
    let mut targets = Vec::new();
    for row in db.list_proxy_provider_secrets()? {
        if !row.enabled {
            continue;
        }
        match list_target_from_row(&row, master_key) {
            Ok(target) => targets.push(target),
            Err(err) => warn!(
                slug = %row.slug,
                error = %err,
                "skipping proxy provider with an unreadable API key"
            ),
        }
    }
    Ok(targets)
}

pub fn create_proxy_provider(
    db: &Database,
    master_key: &str,
    input: ProxyProviderInput,
) -> GatewayResult<ProxyProviderRecord> {
    let slug = normalize_slug(&input.slug)?;
    let base_url = normalize_base_url(&input.base_url)?;
    let label = normalize_label(input.label.as_deref(), &slug)?;
    let (sealed, key_suffix) = seal_optional(master_key, input.api_key.as_deref())?;
    db.insert_proxy_provider(NewProxyProvider {
        slug,
        label,
        base_url,
        api_key_nonce: sealed.as_ref().map(|key| key.nonce.clone()),
        api_key_ciphertext: sealed.map(|key| key.ciphertext),
        key_suffix,
        enabled: input.enabled,
    })
}

pub fn update_proxy_provider(
    db: &Database,
    master_key: &str,
    slug: &str,
    changes: ProxyProviderChanges,
) -> GatewayResult<ProxyProviderRecord> {
    let slug = normalize_slug(slug)?;
    let Some(mut row) = db.get_proxy_provider_secret(&slug)? else {
        return Err(GatewayError::not_found(format!(
            "proxy provider '{slug}' not found"
        )));
    };
    if let Some(label) = changes.label.as_deref() {
        row.label = normalize_label(Some(label), &slug)?;
    }
    if let Some(base_url) = changes.base_url.as_deref() {
        row.base_url = normalize_base_url(base_url)?;
    }
    if let Some(enabled) = changes.enabled {
        row.enabled = enabled;
    }
    if let Some(api_key) = changes.api_key.as_deref() {
        if api_key.is_empty() {
            row.api_key_nonce = None;
            row.api_key_ciphertext = None;
            row.key_suffix = None;
        } else {
            let (sealed, suffix) = seal_optional(master_key, Some(api_key))?;
            let sealed = sealed
                .ok_or_else(|| GatewayError::Internal("proxy API key seal missing".into()))?;
            row.api_key_nonce = Some(sealed.nonce);
            row.api_key_ciphertext = Some(sealed.ciphertext);
            row.key_suffix = suffix;
        }
    }
    db.update_proxy_provider_row(&row)
}

pub fn delete_proxy_provider(db: &Database, slug: &str) -> GatewayResult<String> {
    let slug = normalize_slug(slug)?;
    db.delete_proxy_provider(&slug)?;
    Ok(slug)
}

/// Rewrite `model` and clamp `reasoning_effort` on a chat body.
pub fn prepare_chat_body(body: &mut Value, target: &ProxyTarget, auth: &AuthContext) {
    body["model"] = json!(target.wire_model);
    cap_string_field(body, "reasoning_effort", auth);
}

/// Rewrite `model` and clamp `reasoning.effort` on a Responses body.
pub fn prepare_responses_body(body: &mut Value, target: &ProxyTarget, auth: &AuthContext) {
    body["model"] = json!(target.wire_model);
    if auth.is_master {
        return;
    }
    let Some(max) = auth.max_reasoning_effort.as_deref() else {
        return;
    };
    let Some(effort) = body
        .pointer("/reasoning/effort")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let Some(clamped) = crate::chat::reasoning::clamp_reasoning_effort_str(&effort, max) else {
        return;
    };
    if let Some(reasoning) = body.get_mut("reasoning") {
        reasoning["effort"] = json!(clamped);
    }
}

/// `Authorization` value for the stored upstream key.
#[must_use]
pub fn authorization_value(target: &ProxyTarget) -> Option<String> {
    target
        .api_key
        .as_ref()
        .map(|key| format!("Bearer {}", key.expose_secret()))
}

/// Join a proxy base URL with an absolute API path.
///
/// A query string on the base URL stays on the query, not in the path.
#[must_use]
pub fn join_proxy_url(base: &str, path: &str) -> String {
    let path = path.trim_start_matches('/');
    if let Some((before, query)) = base.split_once('?') {
        let before = before.trim_end_matches('/');
        format!("{before}/{path}?{query}")
    } else {
        join_base_url(base, path)
    }
}

fn cap_string_field(body: &mut Value, field: &str, auth: &AuthContext) {
    if auth.is_master {
        return;
    }
    let Some(max) = auth.max_reasoning_effort.as_deref() else {
        return;
    };
    let Some(effort) = body.get(field).and_then(Value::as_str) else {
        return;
    };
    if let Some(clamped) = crate::chat::reasoning::clamp_reasoning_effort_str(effort, max) {
        body[field] = json!(clamped);
    }
}

fn split_wire_model(rest: &str) -> (bool, String) {
    if let Some(model) = rest.strip_prefix("embedding:")
        && !model.is_empty()
    {
        return (true, model.to_string());
    }
    (false, rest.to_string())
}

fn list_target_from_row(
    row: &ProxyProviderStored,
    master_key: &str,
) -> GatewayResult<ProxyListTarget> {
    Ok(ProxyListTarget {
        slug: row.slug.clone(),
        base_url: row.base_url.clone(),
        api_key: open_stored_key(row, master_key)?,
    })
}

fn open_stored_key(
    row: &ProxyProviderStored,
    master_key: &str,
) -> GatewayResult<Option<SecretString>> {
    match (&row.api_key_nonce, &row.api_key_ciphertext) {
        (Some(nonce), Some(ciphertext)) => Ok(Some(proxy_secret::open_api_key(
            master_key, nonce, ciphertext,
        )?)),
        (None, None) => Ok(None),
        _ => Err(GatewayError::Internal(
            "proxy API key ciphertext is incomplete".into(),
        )),
    }
}

fn seal_optional(
    master_key: &str,
    api_key: Option<&str>,
) -> GatewayResult<(Option<SealedKey>, Option<String>)> {
    let Some(api_key) = api_key.map(str::trim).filter(|key| !key.is_empty()) else {
        return Ok((None, None));
    };
    let suffix = proxy_secret::key_suffix(api_key);
    let sealed = proxy_secret::seal_api_key(master_key, api_key)?;
    Ok((Some(sealed), Some(suffix)))
}

fn normalize_label(label: Option<&str>, slug: &str) -> GatewayResult<String> {
    let Some(label) = label.map(str::trim).filter(|label| !label.is_empty()) else {
        return Ok(slug.to_string());
    };
    if label.len() > 80 {
        return Err(GatewayError::bad_request(
            "label must be 80 characters or fewer",
        ));
    }
    Ok(label.to_string())
}

pub fn normalize_slug(raw: &str) -> GatewayResult<String> {
    let slug = raw.trim().to_ascii_lowercase();
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return Err(GatewayError::bad_request("slug is required"));
    };
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !first.is_ascii_lowercase() || slug.len() > 63 || !rest_ok {
        return Err(GatewayError::bad_request(
            "slug must start with a letter and use only lowercase letters, digits, and hyphens",
        ));
    }
    if ProviderId::from_str(&slug).is_ok() {
        return Err(GatewayError::bad_request(format!(
            "slug '{slug}' is a builtin provider"
        )));
    }
    Ok(slug)
}

pub fn normalize_base_url(raw: &str) -> GatewayResult<String> {
    let url = raw.trim();
    if url.is_empty() || url.chars().any(char::is_whitespace) || url.contains('\\') {
        return Err(GatewayError::bad_request(
            "base_url must be an http or https URL",
        ));
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return Err(GatewayError::bad_request("base_url must use http or https"));
    };
    if rest.contains('#') {
        return Err(GatewayError::bad_request(
            "base_url must not include a fragment",
        ));
    }
    if rest.contains('@') {
        return Err(GatewayError::bad_request(
            "base_url must not include userinfo",
        ));
    }
    let (authority_and_path, query) = match rest.split_once('?') {
        Some((left, query)) => (left, Some(query)),
        None => (rest, None),
    };
    if query.is_some_and(str::is_empty) {
        return Err(GatewayError::bad_request("base_url query is empty"));
    }
    let authority_and_path = authority_and_path.trim_end_matches('/');
    if authority_and_path.is_empty() {
        return Err(GatewayError::bad_request("base_url host is required"));
    }
    let host = authority_and_path.split('/').next().unwrap_or("");
    if host.is_empty() || host.starts_with(':') {
        return Err(GatewayError::bad_request("base_url host is required"));
    }
    let scheme = if url.starts_with("https://") {
        "https"
    } else {
        "http"
    };
    let mut out = format!("{scheme}://{authority_and_path}");
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slug_rejects_builtin_and_bad_shape() {
        assert_eq!(normalize_slug("Home-Lab").unwrap(), "home-lab");
        assert!(normalize_slug("openai").is_err());
        assert!(normalize_slug("9lab").is_err());
        assert!(normalize_slug("has space").is_err());
    }

    #[test]
    fn base_url_allows_private_hosts_and_keeps_the_query() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:11434/v1/").unwrap(),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            normalize_base_url("https://llm.internal/openai?api-version=1").unwrap(),
            "https://llm.internal/openai?api-version=1"
        );
        assert!(normalize_base_url("https://user:pass@host").is_err());
        assert!(normalize_base_url("https://host/path#frag").is_err());
        assert!(normalize_base_url("ftp://host").is_err());
    }

    #[test]
    fn join_keeps_the_query_after_the_path() {
        assert_eq!(
            join_proxy_url("https://llm.internal/openai?api-version=1", "/v1/models"),
            "https://llm.internal/openai/v1/models?api-version=1"
        );
    }

    #[test]
    fn chat_body_rewrites_the_model_and_clamps_effort() {
        let mut body = json!({
            "model": "homelab:llama3.1",
            "reasoning_effort": "xhigh"
        });
        let target = ProxyTarget {
            raw: "homelab:llama3.1".into(),
            wire_model: "llama3.1".into(),
            embedding: false,
            base_url: "http://127.0.0.1:11434".into(),
            api_key: None,
        };
        let auth = crate::gateway::auth::AuthContext {
            key_id: None,
            user_id: "alice".into(),
            is_master: false,
            allowed_models: Some(vec!["homelab:*".into()]),
            max_reasoning_effort: Some("high".into()),
        };
        prepare_chat_body(&mut body, &target, &auth);
        assert_eq!(body["model"], "llama3.1");
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn unknown_prefix_is_a_proxy_model() {
        assert!(is_proxy_model("homelab:llama3.1"));
        assert!(is_proxy_model("homelab:org/model:tag"));
        assert!(!is_proxy_model("openai:gpt-4o-mini"));
        assert!(!is_proxy_model("gpt-4o-mini"));
    }
}
