//! JSON boundary shared by the Python, Node.js, and Kotlin bindings.
//!
//! The config never carries an API key. Bindings pass the credentials of the
//! client that opens the session.

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};

use super::error::VoiceError;
use super::events::VoiceEvent;
use super::session::VoiceConfig;
use crate::providers::{ProviderCredentials, ProviderId};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingVoiceConfig {
    provider: String,
    model: Option<String>,
    voice: Option<String>,
    instructions: Option<String>,
    sample_rate: Option<u32>,
    server_vad: Option<bool>,
    base_url: Option<String>,
    #[serde(default)]
    tools: Vec<BindingVoiceTool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingVoiceTool {
    name: String,
    description: Option<String>,
    #[serde(default = "empty_object_schema")]
    parameters: Value,
}

fn empty_object_schema() -> Value {
    json!({"type": "object"})
}

/// Parse a binding voice config.
///
/// Fields: `provider` (`"openai"` or `"xai"`), `model`, `voice`, `instructions`,
/// `sample_rate`, `server_vad`, `base_url`, and `tools`. OpenAI requires `model`.
pub fn voice_config_from_json(
    json: &str,
    credentials: ProviderCredentials,
) -> Result<VoiceConfig, VoiceError> {
    let raw: BindingVoiceConfig =
        serde_json::from_str(json).map_err(|error| VoiceError::Config(error.to_string()))?;
    let provider: ProviderId = raw
        .provider
        .parse()
        .map_err(|_| VoiceError::UnsupportedProvider)?;
    let mut config = match provider {
        ProviderId::Xai => VoiceConfig::xai(credentials),
        ProviderId::OpenAi => {
            let model = raw
                .model
                .clone()
                .filter(|model| !model.is_empty())
                .ok_or_else(|| {
                    VoiceError::Config("OpenAI voice sessions need a realtime `model`".to_string())
                })?;
            VoiceConfig::openai(credentials, model)
        }
        _ => return Err(VoiceError::UnsupportedProvider),
    };
    if let Some(model) = raw.model.filter(|model| !model.is_empty()) {
        config.model = model;
    }
    if let Some(voice) = raw.voice {
        config.voice = voice;
    }
    if let Some(instructions) = raw.instructions {
        config.instructions = instructions;
    }
    if let Some(sample_rate) = raw.sample_rate {
        if sample_rate == 0 {
            return Err(VoiceError::Config(
                "`sample_rate` must be positive".to_string(),
            ));
        }
        config.sample_rate = sample_rate;
    }
    if let Some(server_vad) = raw.server_vad {
        config.server_vad = server_vad;
    }
    config.base_url = raw.base_url;
    config.tools = raw
        .tools
        .into_iter()
        .map(|tool| crate::tools::ToolSpec {
            name: tool.name,
            description: tool.description,
            parameters_schema: tool.parameters,
            static_tool: false,
        })
        .collect();
    Ok(config)
}

/// Event fields for a binding callback. Audio bytes travel beside this value.
#[must_use]
pub fn voice_event_json(event: &VoiceEvent) -> Value {
    match event {
        VoiceEvent::SessionCreated { session_id } => {
            json!({"type": "session_created", "session_id": session_id})
        }
        VoiceEvent::SessionUpdated => json!({"type": "session_updated"}),
        VoiceEvent::SpeechStarted => json!({"type": "speech_started"}),
        VoiceEvent::SpeechStopped => json!({"type": "speech_stopped"}),
        VoiceEvent::InputTranscript { text, final_ } => {
            json!({"type": "input_transcript", "text": text, "final": final_})
        }
        VoiceEvent::OutputAudio(audio) => json!({"type": "output_audio", "bytes": audio.len()}),
        VoiceEvent::OutputTranscript { text, final_ } => {
            json!({"type": "output_transcript", "text": text, "final": final_})
        }
        VoiceEvent::ResponseDone => json!({"type": "response_done"}),
        VoiceEvent::FunctionCall {
            call_id,
            name,
            arguments,
        } => json!({
            "type": "function_call",
            "call_id": call_id,
            "name": name,
            "arguments": arguments
        }),
        VoiceEvent::Interrupted => json!({"type": "interrupted"}),
        VoiceEvent::Error { message } => json!({"type": "error", "message": message}),
        VoiceEvent::Provider {
            event_type,
            payload,
        } => json!({"type": "provider", "event_type": event_type, "payload": payload}),
        VoiceEvent::Closed => json!({"type": "closed"}),
    }
}

/// Assistant audio carried by an event, if any.
#[must_use]
pub fn voice_event_audio(event: &VoiceEvent) -> Option<&Bytes> {
    match event {
        VoiceEvent::OutputAudio(audio) => Some(audio),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_config_needs_model_and_keeps_fields() {
        let missing =
            voice_config_from_json(r#"{"provider":"openai"}"#, ProviderCredentials::new());
        assert!(matches!(missing, Err(VoiceError::Config(_))));

        let config = voice_config_from_json(
            r#"{"provider":"openai","model":"gpt-realtime","voice":"cedar","sample_rate":24000,
                "tools":[{"name":"lookup"}]}"#,
            ProviderCredentials::new(),
        )
        .unwrap();
        assert_eq!(config.provider, ProviderId::OpenAi);
        assert_eq!(config.model, "gpt-realtime");
        assert_eq!(config.voice, "cedar");
        assert_eq!(config.tools[0].parameters_schema, json!({"type": "object"}));
    }

    #[test]
    fn rejects_unknown_fields_and_unsupported_providers() {
        let key = voice_config_from_json(
            r#"{"provider":"xai","api_key":"secret"}"#,
            ProviderCredentials::new(),
        );
        let Err(VoiceError::Config(message)) = key else {
            panic!("an api_key field must be rejected");
        };
        assert!(!message.contains("secret"));

        assert!(matches!(
            voice_config_from_json(r#"{"provider":"anthropic"}"#, ProviderCredentials::new()),
            Err(VoiceError::UnsupportedProvider)
        ));
        assert!(matches!(
            voice_config_from_json(r#"{"provider":"nope"}"#, ProviderCredentials::new()),
            Err(VoiceError::UnsupportedProvider)
        ));
    }

    #[test]
    fn event_json_keeps_audio_beside_fields() {
        let audio = VoiceEvent::OutputAudio(Bytes::from_static(b"pcm"));
        assert_eq!(
            voice_event_json(&audio),
            json!({"type": "output_audio", "bytes": 3})
        );
        assert_eq!(voice_event_audio(&audio).map(Bytes::len), Some(3));

        let call = VoiceEvent::FunctionCall {
            call_id: "c1".into(),
            name: "lookup".into(),
            arguments: "{}".into(),
        };
        assert_eq!(voice_event_json(&call)["type"], "function_call");
        assert!(voice_event_audio(&call).is_none());
    }
}
