//! Bedrock runtime and control-plane URLs, InvokeModel bodies, and catalog parsing.
//!
//! Chat stays on the OpenAI-compatible base URL. Embeddings and the model list
//! use the hosts derived from that URL.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::credentials::is_aws_region;

/// Titan `InvokeModel` accepts one string per request.
pub(crate) const TITAN_MAX_TEXTS: usize = 32;
/// Cohere Embed accepts one batch of this size.
pub(crate) const COHERE_MAX_TEXTS: usize = 96;
/// Bound on `ListInferenceProfiles` pages.
pub(crate) const MAX_PROFILE_PAGES: usize = 10;

const MAX_LIST_TOKEN_LEN: usize = 2048;

/// Runtime host for `InvokeModel`, and control host for the model list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BedrockHosts {
    pub runtime: String,
    pub control: String,
}

/// One model id from `ListFoundationModels` or `ListInferenceProfiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListedModel {
    pub id: String,
    pub embedding: bool,
}

/// InvokeModel body family selected by the model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbedFamily {
    TitanText,
    Cohere,
}

/// One Titan embedding and its input token count.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TitanVector {
    pub vector: Vec<f32>,
    pub input_tokens: Option<u64>,
}

/// The model id cannot be placed in the invoke path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidModelId;

/// `ListInferenceProfiles` returned a page token that cannot be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidListToken;

/// Split a chat base URL into the runtime root and the catalog host.
///
/// `https://bedrock-runtime.{region}.amazonaws.com/openai` uses
/// `https://bedrock.{region}.amazonaws.com` for the catalog. Any other base
/// URL keeps the runtime root for both calls.
#[must_use]
pub(crate) fn bedrock_hosts(openai_base: &str) -> BedrockHosts {
    let trimmed = openai_base.trim().trim_end_matches('/');
    let runtime = trimmed
        .strip_suffix("/openai")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string();
    let control = control_root(&runtime).unwrap_or_else(|| runtime.clone());
    BedrockHosts { runtime, control }
}

/// `POST {runtime}/model/{modelId}/invoke` with the model id encoded as one segment.
pub(crate) fn invoke_url(openai_base: &str, model_id: &str) -> Result<String, InvalidModelId> {
    let hosts = bedrock_hosts(openai_base);
    if hosts.runtime.is_empty() {
        return Err(InvalidModelId);
    }
    let encoded = encode_model_id(model_id)?;
    Ok(format!("{}/model/{encoded}/invoke", hosts.runtime))
}

#[must_use]
pub(crate) fn foundation_models_url(control: &str) -> String {
    format!(
        "{}/foundation-models?byInferenceType=ON_DEMAND",
        control.trim_end_matches('/')
    )
}

pub(crate) fn inference_profiles_url(
    control: &str,
    next_token: Option<&str>,
) -> Result<String, InvalidListToken> {
    let mut url = format!(
        "{}/inference-profiles?maxResults=100",
        control.trim_end_matches('/')
    );
    if let Some(token) = next_token {
        if !valid_list_token(token) {
            return Err(InvalidListToken);
        }
        url.push_str("&nextToken=");
        url.push_str(&percent_encode(token));
    }
    Ok(url)
}

/// Read `nextToken`. An empty or missing token ends the list.
pub(crate) fn page_token(body: &Value) -> Result<Option<String>, InvalidListToken> {
    match body.get("nextToken") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(token)) if token.is_empty() => Ok(None),
        Some(Value::String(token)) if valid_list_token(token) => Ok(Some(token.clone())),
        Some(_) => Err(InvalidListToken),
    }
}

/// Model id selects the InvokeModel JSON body.
///
/// `titan-embed-image` is rejected. `titan-embed` is Titan text. `cohere.embed` is Cohere.
pub(crate) fn embed_family(model_id: &str) -> Result<EmbedFamily, InvalidModelId> {
    let id = model_id.to_ascii_lowercase();
    if id.contains("titan-embed-image") {
        return Err(InvalidModelId);
    }
    if id.contains("titan-embed") {
        return Ok(EmbedFamily::TitanText);
    }
    if id.contains("cohere.embed") {
        return Ok(EmbedFamily::Cohere);
    }
    Err(InvalidModelId)
}

#[must_use]
pub(crate) fn titan_request_body(text: &str, dimensions: Option<usize>) -> Value {
    let mut body = json!({ "inputText": text });
    if let Some(dimensions) = dimensions {
        body["dimensions"] = json!(dimensions);
    }
    body
}

#[must_use]
pub(crate) fn cohere_request_body(texts: &[String]) -> Value {
    json!({
        "texts": texts,
        "input_type": "search_document",
    })
}

pub(crate) fn parse_titan_response(value: &Value) -> Result<TitanVector, ()> {
    let vector = f32_array(value.get("embedding").ok_or(())?)?;
    if vector.is_empty() {
        return Err(());
    }
    let input_tokens = value.get("inputTextTokenCount").and_then(Value::as_u64);
    Ok(TitanVector {
        vector,
        input_tokens,
    })
}

pub(crate) fn parse_cohere_response(value: &Value) -> Result<Vec<Vec<f32>>, ()> {
    let embeddings = value.get("embeddings").ok_or(())?;
    let rows = if let Some(rows) = embeddings.as_array() {
        rows
    } else {
        embeddings
            .get("float")
            .and_then(Value::as_array)
            .ok_or(())?
    };
    if rows.is_empty() {
        return Err(());
    }
    rows.iter()
        .map(|row| {
            let vector = f32_array(row)?;
            if vector.is_empty() {
                Err(())
            } else {
                Ok(vector)
            }
        })
        .collect()
}

/// Foundation models, then inference profiles.
///
/// A profile is an embedding model when its id names an embedding foundation
/// model, or when a member ARN does.
pub(crate) fn catalog_models(
    foundation: &Value,
    profile_pages: &[Value],
) -> Result<Vec<ListedModel>, ()> {
    let summaries = foundation
        .get("modelSummaries")
        .and_then(Value::as_array)
        .ok_or(())?;
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    let mut embedding_ids = HashSet::new();
    for summary in summaries {
        let Some(id) = summary.get("modelId").and_then(Value::as_str) else {
            continue;
        };
        if !usable_model_id(id) {
            continue;
        }
        let embedding = output_is_embedding(summary);
        if embedding {
            embedding_ids.insert(id.to_string());
        }
        push_model(
            &mut models,
            &mut seen,
            ListedModel {
                id: id.to_string(),
                embedding,
            },
        );
    }

    for page in profile_pages {
        let profiles = page
            .get("inferenceProfileSummaries")
            .and_then(Value::as_array)
            .ok_or(())?;
        for profile in profiles {
            let Some(id) = profile.get("inferenceProfileId").and_then(Value::as_str) else {
                continue;
            };
            if !usable_model_id(id) {
                continue;
            }
            let embedding = profile_is_embedding(id, profile.get("models"), &embedding_ids);
            push_model(
                &mut models,
                &mut seen,
                ListedModel {
                    id: id.to_string(),
                    embedding,
                },
            );
        }
    }
    Ok(models)
}

fn control_root(runtime: &str) -> Option<String> {
    let host = runtime.strip_prefix("https://")?;
    let host = host.split_once('/').map(|(host, _)| host).unwrap_or(host);
    if host.contains(':') || host.contains('@') {
        return None;
    }
    let region = host
        .strip_prefix("bedrock-runtime.")?
        .strip_suffix(".amazonaws.com")?;
    if !is_aws_region(region) {
        return None;
    }
    Some(format!("https://bedrock.{region}.amazonaws.com"))
}

fn encode_model_id(model_id: &str) -> Result<String, InvalidModelId> {
    if !usable_model_id(model_id) {
        return Err(InvalidModelId);
    }
    Ok(percent_encode(model_id))
}

fn usable_model_id(model_id: &str) -> bool {
    !model_id.is_empty() && !model_id.contains(['/', '?', '#', '\\'])
}

fn valid_list_token(token: &str) -> bool {
    (1..=MAX_LIST_TOKEN_LEN).contains(&token.len())
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_' | b'.' | b'~')
        })
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn f32_array(value: &Value) -> Result<Vec<f32>, ()> {
    value
        .as_array()
        .ok_or(())?
        .iter()
        .map(|number| {
            let number = number.as_f64().ok_or(())?;
            if !number.is_finite() {
                return Err(());
            }
            Ok(number as f32)
        })
        .collect()
}

fn output_is_embedding(summary: &Value) -> bool {
    summary
        .get("outputModalities")
        .and_then(Value::as_array)
        .is_some_and(|modalities| {
            modalities
                .iter()
                .any(|modality| modality.as_str() == Some("EMBEDDING"))
        })
}

fn profile_is_embedding(id: &str, models: Option<&Value>, embedding_ids: &HashSet<String>) -> bool {
    if embedding_ids.iter().any(|foundation| {
        foundation == id
            || id
                .strip_suffix(foundation.as_str())
                .is_some_and(|prefix| prefix.ends_with('.'))
    }) {
        return true;
    }
    let Some(models) = models.and_then(Value::as_array) else {
        return false;
    };
    models.iter().any(|model| {
        model
            .get("modelArn")
            .and_then(Value::as_str)
            .and_then(|arn| arn.rsplit_once('/'))
            .is_some_and(|(_, model_id)| embedding_ids.contains(model_id))
    })
}

fn push_model(models: &mut Vec<ListedModel>, seen: &mut HashSet<String>, model: ListedModel) {
    if !seen.insert(model.id.clone()) {
        if model.embedding
            && let Some(existing) = models.iter_mut().find(|item| item.id == model.id)
        {
            existing.embedding = true;
        }
        return;
    }
    models.push(model);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_url_encodes_colon_in_model_id() {
        let url = invoke_url(
            "https://bedrock-runtime.us-east-1.amazonaws.com/openai",
            "amazon.titan-embed-text-v2:0",
        )
        .expect("model id");
        assert_eq!(
            url,
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.titan-embed-text-v2%3A0/invoke"
        );
    }

    #[test]
    fn public_runtime_base_selects_control_host() {
        let hosts = bedrock_hosts("https://bedrock-runtime.us-west-2.amazonaws.com/openai/");
        assert_eq!(
            hosts.runtime,
            "https://bedrock-runtime.us-west-2.amazonaws.com"
        );
        assert_eq!(hosts.control, "https://bedrock.us-west-2.amazonaws.com");
        assert_eq!(
            foundation_models_url(&hosts.control),
            "https://bedrock.us-west-2.amazonaws.com/foundation-models?byInferenceType=ON_DEMAND"
        );
    }

    #[test]
    fn custom_base_stays_on_that_server() {
        let hosts = bedrock_hosts("http://127.0.0.1:9/openai");
        assert_eq!(hosts.runtime, "http://127.0.0.1:9");
        assert_eq!(hosts.control, "http://127.0.0.1:9");
        assert_eq!(
            invoke_url("http://127.0.0.1:9/openai", "cohere.embed-english-v3").expect("id"),
            "http://127.0.0.1:9/model/cohere.embed-english-v3/invoke"
        );
    }

    #[test]
    fn model_id_with_slash_is_rejected() {
        assert!(invoke_url("https://example.test/openai", "amazon/titan").is_err());
        assert!(invoke_url("https://example.test/openai", "").is_err());
        assert!(invoke_url("https://example.test/openai", "a?b").is_err());
    }

    #[test]
    fn embed_family_selects_titan_cohere_and_rejects_image() {
        assert_eq!(
            embed_family("amazon.titan-embed-text-v2:0").expect("titan"),
            EmbedFamily::TitanText
        );
        assert_eq!(
            embed_family("us.amazon.titan-embed-text-v2:0").expect("profile"),
            EmbedFamily::TitanText
        );
        assert_eq!(
            embed_family("cohere.embed-english-v3").expect("cohere"),
            EmbedFamily::Cohere
        );
        assert_eq!(
            embed_family("us.cohere.embed-v4:0").expect("cohere profile"),
            EmbedFamily::Cohere
        );
        assert!(embed_family("amazon.titan-embed-image-v1").is_err());
        assert!(embed_family("anthropic.claude-sonnet-4").is_err());
    }

    #[test]
    fn titan_and_cohere_bodies_match_invoke_model() {
        assert_eq!(
            titan_request_body("hello", Some(256)),
            json!({"inputText": "hello", "dimensions": 256})
        );
        assert_eq!(
            titan_request_body("hello", None),
            json!({"inputText": "hello"})
        );
        assert_eq!(
            cohere_request_body(&["a".into(), "b".into()]),
            json!({"texts": ["a", "b"], "input_type": "search_document"})
        );
    }

    #[test]
    fn parses_titan_and_cohere_vectors() {
        let titan = parse_titan_response(&json!({
            "embedding": [0.5, 1.5],
            "inputTextTokenCount": 4
        }))
        .expect("titan");
        assert_eq!(titan.vector, vec![0.5, 1.5]);
        assert_eq!(titan.input_tokens, Some(4));

        let rows = parse_cohere_response(&json!({"embeddings": [[0.25], [0.75]]})).expect("v3");
        assert_eq!(rows, vec![vec![0.25], vec![0.75]]);

        let v4 = parse_cohere_response(&json!({
            "embeddings": {"float": [[1.0, 2.0]]}
        }))
        .expect("v4");
        assert_eq!(v4, vec![vec![1.0, 2.0]]);
        assert!(parse_cohere_response(&json!({"embeddings": []})).is_err());
    }

    #[test]
    fn catalog_marks_embedding_models_and_profiles() {
        let foundation = json!({
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
        });
        let page = json!({
            "inferenceProfileSummaries": [
                {
                    "inferenceProfileId": "us.amazon.titan-embed-text-v2:0",
                    "models": [{
                        "modelArn": "arn:aws:bedrock:us-east-1::foundation-model/amazon.titan-embed-text-v2:0"
                    }]
                },
                {
                    "inferenceProfileId": "us.anthropic.claude-3-haiku-20240307-v1:0",
                    "models": [{
                        "modelArn": "arn:aws:bedrock:us-east-1::foundation-model/anthropic.claude-3-haiku-20240307-v1:0"
                    }]
                }
            ]
        });
        let models = catalog_models(&foundation, &[page]).expect("catalog");
        assert_eq!(
            models,
            vec![
                ListedModel {
                    id: "amazon.titan-embed-text-v2:0".into(),
                    embedding: true,
                },
                ListedModel {
                    id: "anthropic.claude-3-haiku-20240307-v1:0".into(),
                    embedding: false,
                },
                ListedModel {
                    id: "us.amazon.titan-embed-text-v2:0".into(),
                    embedding: true,
                },
                ListedModel {
                    id: "us.anthropic.claude-3-haiku-20240307-v1:0".into(),
                    embedding: false,
                },
            ]
        );
    }

    #[test]
    fn page_token_is_encoded_and_unsafe_tokens_are_rejected() {
        let url = inference_profiles_url("https://bedrock.us-east-1.amazonaws.com", Some("ab/cd+"))
            .expect("token");
        assert_eq!(
            url,
            "https://bedrock.us-east-1.amazonaws.com/inference-profiles?maxResults=100&nextToken=ab%2Fcd%2B"
        );
        assert!(page_token(&json!({"nextToken": "a&b"})).is_err());
        assert!(
            page_token(&json!({"nextToken": "ok"}))
                .expect("token")
                .is_some()
        );
        assert!(page_token(&json!({})).expect("end").is_none());
        let long = "a".repeat(MAX_LIST_TOKEN_LEN + 1);
        assert!(page_token(&json!({"nextToken": long})).is_err());
    }
}
