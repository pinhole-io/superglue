//! xAI Speech to Speech WebSocket adapter.

use bytes::Bytes;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use super::error::VoiceError;
use super::provider::{
    PlaybackState, VoiceProvider, VoiceWebSocket, connect_bearer, tool_definitions,
};
use super::session::VoiceConfig;

pub(crate) struct XaiVoiceProvider;

#[async_trait::async_trait]
impl VoiceProvider for XaiVoiceProvider {
    fn provider_id(&self) -> crate::providers::ProviderId {
        crate::providers::ProviderId::Xai
    }

    async fn connect(&self, config: &VoiceConfig) -> Result<VoiceWebSocket, VoiceError> {
        let mut query = vec![("model", config.model.as_str())];
        if let Some(conversation_id) = &config.conversation_id {
            query.push(("conversation_id", conversation_id.as_str()));
        }
        connect_bearer(crate::providers::ProviderId::Xai, config, &query).await
    }

    fn session_update(&self, config: &VoiceConfig) -> Value {
        session_update(config)
    }

    /// xAI takes caller audio as binary WebSocket frames.
    fn encode_audio(&self, audio: Bytes) -> Message {
        Message::Binary(audio)
    }

    fn interrupt(&self, _playback: &PlaybackState) -> Vec<Value> {
        vec![json!({"type": "response.cancel"})]
    }
}

pub(crate) fn session_update(config: &VoiceConfig) -> Value {
    let mut session = json!({
        "voice": config.voice,
        "instructions": config.instructions,
        "turn_detection": {
            "type": if config.server_vad { "server_vad" } else { "none" }
        },
        "audio": {
            "input": {
                "format": {
                    "type": config.input_format,
                    "rate": config.sample_rate
                },
                "transport": "binary"
            },
            "output": {
                "format": {
                    "type": config.output_format,
                    "rate": config.sample_rate
                },
                "transport": "binary"
            }
        },
        "resumption": { "enabled": config.resumption_enabled }
    });
    if !config.tools.is_empty() {
        session["tools"] = tool_definitions(&config.tools);
    }
    json!({ "type": "session.update", "session": session })
}
