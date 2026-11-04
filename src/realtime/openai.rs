//! OpenAI Realtime WebSocket adapter.

use base64::Engine;
use bytes::Bytes;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::error::VoiceError;
use super::provider::{
    PlaybackState, VoiceProvider, VoiceWebSocket, connect_bearer, tool_definitions,
};
use super::session::VoiceConfig;

pub(crate) struct OpenAiVoiceProvider;

#[async_trait::async_trait]
impl VoiceProvider for OpenAiVoiceProvider {
    fn provider_id(&self) -> crate::providers::ProviderId {
        crate::providers::ProviderId::OpenAi
    }

    async fn connect(&self, config: &VoiceConfig) -> Result<VoiceWebSocket, VoiceError> {
        connect_bearer(
            crate::providers::ProviderId::OpenAi,
            config,
            &[("model", config.model.as_str())],
        )
        .await
    }

    fn session_update(&self, config: &VoiceConfig) -> Value {
        session_update(config)
    }

    /// OpenAI takes caller audio as base64 PCM inside `input_audio_buffer.append`.
    fn encode_audio(&self, audio: Bytes) -> Message {
        Message::Text(audio_append(&audio).to_string().into())
    }

    /// Cancel the response, then cut the assistant item to the audio already delivered,
    /// so the server transcript matches what the caller received.
    fn interrupt(&self, playback: &PlaybackState) -> Vec<Value> {
        let mut events = vec![json!({"type": "response.cancel"})];
        if let Some(item_id) = &playback.item_id {
            events.push(json!({
                "type": "conversation.item.truncate",
                "item_id": item_id,
                "content_index": 0,
                "audio_end_ms": playback.audio_ms
            }));
        }
        events
    }
}

pub(crate) fn audio_append(audio: &[u8]) -> Value {
    json!({
        "type": "input_audio_buffer.append",
        "audio": base64::engine::general_purpose::STANDARD.encode(audio)
    })
}

pub(crate) fn session_update(config: &VoiceConfig) -> Value {
    let turn_detection = if config.server_vad {
        json!({ "type": "server_vad" })
    } else {
        Value::Null
    };
    let mut session = json!({
        "type": "realtime",
        "model": config.model,
        "instructions": config.instructions,
        "output_modalities": ["audio"],
        "audio": {
            "input": {
                "format": {
                    "type": config.input_format,
                    "rate": config.sample_rate
                },
                "turn_detection": turn_detection
            },
            "output": {
                "format": {
                    "type": config.output_format,
                    "rate": config.sample_rate
                },
                "voice": config.voice
            }
        }
    });
    if !config.tools.is_empty() {
        session["tools"] = tool_definitions(&config.tools);
    }
    json!({ "type": "session.update", "session": session })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ProviderCredentials, ProviderId};
    use crate::realtime::resolve_voice_provider;

    fn config() -> VoiceConfig {
        let mut config = VoiceConfig::openai(ProviderCredentials::new(), "gpt-realtime");
        config.tools = vec![crate::tools::ToolSpec {
            name: "lookup".into(),
            description: Some("find a record".into()),
            parameters_schema: json!({"type": "object"}),
            static_tool: false,
        }];
        config
    }

    #[test]
    fn session_update_uses_ga_shape() {
        let update = session_update(&config());
        assert_eq!(update["type"], "session.update");
        let session = &update["session"];
        assert_eq!(session["type"], "realtime");
        assert_eq!(session["model"], "gpt-realtime");
        assert_eq!(session["audio"]["input"]["format"]["type"], "audio/pcm");
        assert_eq!(session["audio"]["input"]["format"]["rate"], 24_000);
        assert_eq!(
            session["audio"]["input"]["turn_detection"]["type"],
            "server_vad"
        );
        assert_eq!(session["audio"]["output"]["voice"], "marin");
        assert_eq!(session["tools"][0]["name"], "lookup");
        assert!(session.get("resumption").is_none());
    }

    #[test]
    fn session_update_without_vad_sends_null_turn_detection() {
        let mut config = config();
        config.server_vad = false;
        let update = session_update(&config);
        assert!(update["session"]["audio"]["input"]["turn_detection"].is_null());
    }

    #[test]
    fn audio_is_base64_text_frame() {
        let message = OpenAiVoiceProvider.encode_audio(Bytes::from_static(b"pcm"));
        let Message::Text(text) = message else {
            panic!("OpenAI audio must be a text frame");
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["type"], "input_audio_buffer.append");
        assert_eq!(value["audio"], "cGNt");
    }

    #[test]
    fn interrupt_cancels_and_truncates_delivered_audio() {
        let events = OpenAiVoiceProvider.interrupt(&PlaybackState {
            item_id: Some("item-1".into()),
            audio_ms: 250,
        });
        assert_eq!(events[0]["type"], "response.cancel");
        assert_eq!(events[1]["type"], "conversation.item.truncate");
        assert_eq!(events[1]["item_id"], "item-1");
        assert_eq!(events[1]["audio_end_ms"], 250);

        let idle = OpenAiVoiceProvider.interrupt(&PlaybackState::default());
        assert_eq!(idle.len(), 1);
    }

    #[test]
    fn xai_keeps_binary_audio_and_plain_cancel() {
        let xai = resolve_voice_provider(ProviderId::Xai).unwrap();
        assert!(matches!(
            xai.encode_audio(Bytes::from_static(b"pcm")),
            Message::Binary(_)
        ));
        let events = xai.interrupt(&PlaybackState {
            item_id: Some("item-1".into()),
            audio_ms: 250,
        });
        assert_eq!(events, vec![json!({"type": "response.cancel"})]);
    }

    #[test]
    fn resolves_openai_and_rejects_other_providers() {
        assert_eq!(
            resolve_voice_provider(ProviderId::OpenAi)
                .unwrap()
                .provider_id(),
            ProviderId::OpenAi
        );
        assert!(matches!(
            resolve_voice_provider(ProviderId::Anthropic),
            Err(VoiceError::UnsupportedProvider)
        ));
    }
}
