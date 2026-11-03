//! Offline file transcription proxy.
//!
//! Public paths: `/v1/audio/transcriptions` and `/v1/speech/transcriptions`.
//! OpenAI and Groq receive `/v1/audio/transcriptions`. xAI receives `/v1/stt`.
//!
//! Do not log audio bytes or transcript text.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, State};
use axum::response::{IntoResponse, Response};
use secrecy::ExposeSecret;

use serde_json::Value;

use crate::audio::{provider_transcription_path, supports_transcription};
use crate::gateway::GatewayState;
use crate::gateway::auth::{Auth, resolve_user_id};
use crate::gateway::error::{GatewayError, GatewayResult};
use crate::gateway::proxy::preflight_async;
use crate::http::join_base_url;
use crate::providers::{ProviderId, parse_model_ref};

const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;

struct TranscriptionForm {
    model: String,
    user: Option<String>,
    fields: Vec<(String, String)>,
    filename: String,
    file_bytes: Vec<u8>,
    mime: String,
}

/// Proxy an OpenAI-compatible transcription request to the upstream provider.
pub async fn transcribe(
    State(state): State<Arc<GatewayState>>,
    Auth(auth): Auth,
    multipart: Multipart,
) -> Result<Response, GatewayError> {
    let form = parse_form(multipart).await?;
    let qualified = qualify_transcription_model(&form.model);
    let model_ref = parse_model_ref(&qualified);
    if !supports_transcription(model_ref.provider) {
        return Err(GatewayError::bad_request(format!(
            "file transcription is not supported for {}",
            model_ref.provider
        )));
    }
    let user_id = resolve_user_id(&auth, form.user.as_deref())?;
    preflight_async(&state.db, &auth, &user_id, &qualified).await?;

    let key = state
        .credentials
        .key_for(model_ref.provider)
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    let base_url = state.credentials.base_url_for(model_ref.provider);
    let url = join_base_url(&base_url, provider_transcription_path(model_ref.provider));
    let auth_header = format!("Bearer {}", key.expose_secret());

    let mut upstream_fields = form.fields;
    if model_ref.provider == ProviderId::Xai {
        upstream_fields.retain(|(name, _)| !openai_only_field(name));
    }
    upstream_fields.retain(|(name, _)| name != "model");
    upstream_fields.insert(0, ("model".to_string(), model_ref.model.clone()));
    let field_refs: Vec<(&str, &str)> = upstream_fields
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    let value = state
        .http
        .post_multipart_with_fields(
            &url,
            &[("Authorization", auth_header.as_str())],
            &field_refs,
            &form.filename,
            form.file_bytes,
            &form.mime,
            None,
        )
        .await
        .map_err(|error| GatewayError::upstream(error.to_string()))?;
    Ok(Json(openai_word_shape(model_ref.provider, value)).into_response())
}

fn openai_only_field(name: &str) -> bool {
    matches!(
        name,
        "response_format" | "timestamp_granularities" | "timestamp_granularities[]"
    )
}

/// xAI returns `words[].text`. OpenAI clients read `words[].word`.
fn openai_word_shape(provider: ProviderId, mut value: Value) -> Value {
    if provider != ProviderId::Xai {
        return value;
    }
    let Some(words) = value.get_mut("words").and_then(Value::as_array_mut) else {
        return value;
    };
    for word in words {
        let Some(object) = word.as_object_mut() else {
            continue;
        };
        if object.contains_key("word") {
            object.remove("text");
            continue;
        }
        if let Some(text) = object.remove("text") {
            object.insert("word".to_string(), text);
        }
    }
    value
}

fn qualify_transcription_model(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.contains(':') {
        return trimmed.to_string();
    }
    if trimmed.starts_with("grok-voice") {
        return format!("xai:{trimmed}");
    }
    let model_ref = parse_model_ref(trimmed);
    format!("{}:{}", model_ref.provider.as_str(), model_ref.model)
}

async fn parse_form(mut multipart: Multipart) -> GatewayResult<TranscriptionForm> {
    let mut model = None;
    let mut user = None;
    let mut fields = Vec::new();
    let mut file = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| GatewayError::bad_request(error.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let filename = field
                .file_name()
                .filter(|name| !name.is_empty())
                .unwrap_or("audio.wav")
                .to_string();
            let mime = field
                .content_type()
                .unwrap_or("application/octet-stream")
                .to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|error| GatewayError::bad_request(error.to_string()))?;
            if bytes.len() > MAX_AUDIO_BYTES {
                return Err(GatewayError::payload_too_large("audio body exceeds 32 MiB"));
            }
            if bytes.is_empty() {
                return Err(GatewayError::bad_request("audio file is empty"));
            }
            file = Some((filename, bytes.to_vec(), mime));
            continue;
        }

        let text = field
            .text()
            .await
            .map_err(|error| GatewayError::bad_request(error.to_string()))?;
        match name.as_str() {
            "model" => model = Some(text),
            "user" => user = Some(text).filter(|value| !value.is_empty()),
            _ if !name.is_empty() => fields.push((name, text)),
            _ => {}
        }
    }

    let model = model
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GatewayError::bad_request("model is required"))?;
    let (filename, file_bytes, mime) =
        file.ok_or_else(|| GatewayError::bad_request("file is required"))?;
    Ok(TranscriptionForm {
        model,
        user,
        fields,
        filename,
        file_bytes,
        mime,
    })
}

#[cfg(test)]
mod tests {
    use super::qualify_transcription_model;

    #[test]
    fn qualify_adds_openai_prefix_to_bare_whisper() {
        assert_eq!(qualify_transcription_model("whisper-1"), "openai:whisper-1");
        assert_eq!(
            qualify_transcription_model("openai:whisper-1"),
            "openai:whisper-1"
        );
    }

    #[test]
    fn qualify_maps_bare_grok_voice_to_xai() {
        assert_eq!(
            qualify_transcription_model("grok-voice-transcribe-2.0"),
            "xai:grok-voice-transcribe-2.0"
        );
        assert_eq!(
            qualify_transcription_model("xai:grok-voice-transcribe-2.0"),
            "xai:grok-voice-transcribe-2.0"
        );
    }

    #[test]
    fn xai_words_use_the_openai_field_name() {
        use super::openai_word_shape;
        use crate::providers::ProviderId;
        use serde_json::json;

        let shaped = openai_word_shape(
            ProviderId::Xai,
            json!({
                "text": "hello",
                "language": "en",
                "duration": 0.4,
                "words": [{"text": "hello", "start": 0.0, "end": 0.4}]
            }),
        );
        assert_eq!(shaped["text"], "hello");
        assert_eq!(shaped["language"], "en");
        assert_eq!(shaped["words"][0]["word"], "hello");
        assert!(shaped["words"][0].get("text").is_none());
    }
}
