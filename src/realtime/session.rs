//! A provider-neutral handle for one duplex voice session.

use base64::Engine;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::error::VoiceError;
use super::events::{VoiceClientEvent, VoiceEvent};
use super::provider::{PlaybackState, VoiceProvider, resolve_voice_provider};

/// Configuration shared by realtime provider adapters.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub provider: crate::providers::ProviderId,
    pub model: String,
    pub voice: String,
    pub instructions: String,
    pub input_format: String,
    pub output_format: String,
    pub sample_rate: u32,
    pub server_vad: bool,
    pub resumption_enabled: bool,
    pub conversation_id: Option<String>,
    pub tools: Vec<crate::tools::ToolSpec>,
    pub credentials: crate::providers::ProviderCredentials,
    pub base_url: Option<String>,
    pub cancel: CancellationToken,
    pub status_emitter: Option<std::sync::Arc<crate::events::StatusEmitter>>,
}

impl VoiceConfig {
    #[must_use]
    pub fn xai(credentials: crate::providers::ProviderCredentials) -> Self {
        Self {
            provider: crate::providers::ProviderId::Xai,
            model: "grok-voice-latest".to_string(),
            voice: "eve".to_string(),
            instructions: "You are a helpful assistant.".to_string(),
            input_format: "audio/pcm".to_string(),
            output_format: "audio/pcm".to_string(),
            sample_rate: 24_000,
            server_vad: true,
            resumption_enabled: true,
            conversation_id: None,
            tools: Vec::new(),
            credentials,
            base_url: None,
            cancel: CancellationToken::new(),
            status_emitter: None,
        }
    }

    /// OpenAI Realtime over WebSocket. `model` is a realtime model such as `gpt-realtime`.
    #[must_use]
    pub fn openai(
        credentials: crate::providers::ProviderCredentials,
        model: impl Into<String>,
    ) -> Self {
        Self {
            provider: crate::providers::ProviderId::OpenAi,
            model: model.into(),
            voice: "marin".to_string(),
            instructions: "You are a helpful assistant.".to_string(),
            input_format: "audio/pcm".to_string(),
            output_format: "audio/pcm".to_string(),
            sample_rate: 24_000,
            server_vad: true,
            resumption_enabled: false,
            conversation_id: None,
            tools: Vec::new(),
            credentials,
            base_url: None,
            cancel: CancellationToken::new(),
            status_emitter: None,
        }
    }
}

/// A connected session. The handle is safe to move between tasks.
#[derive(Clone)]
pub struct VoiceSession {
    commands: mpsc::Sender<VoiceClientEvent>,
}

impl std::fmt::Debug for VoiceSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoiceSession").finish_non_exhaustive()
    }
}

/// The receiving side of a [`VoiceSession`].
pub struct VoiceEvents {
    receiver: mpsc::Receiver<VoiceEvent>,
}

impl VoiceEvents {
    pub async fn recv(&mut self) -> Option<VoiceEvent> {
        self.receiver.recv().await
    }

    /// Wait for the next event on a thread outside the async runtime.
    ///
    /// # Panics
    ///
    /// Panics when called from an async execution context.
    pub fn blocking_recv(&mut self) -> Option<VoiceEvent> {
        self.receiver.blocking_recv()
    }
}

impl VoiceSession {
    /// Connect to the configured provider and start its event pump.
    ///
    /// The returned receiver owns the provider event stream. Dropping it stops
    /// delivery, while calling [`Self::close`] closes the provider connection.
    pub async fn connect(config: VoiceConfig) -> Result<(Self, VoiceEvents), VoiceError> {
        let provider = resolve_voice_provider(config.provider)?;
        let mut socket = provider.connect(&config).await?;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                provider.session_update(&config).to_string().into(),
            ))
            .await?;

        let (mut writer, mut reader) = socket.split();
        let (command_tx, mut command_rx) = mpsc::channel(64);
        let (event_tx, event_rx) = mpsc::channel(256);
        let cancel = config.cancel.clone();
        let status_emitter = config.status_emitter.clone();
        let model = config.model.clone();
        let sample_rate = config.sample_rate;
        let request_id = uuid::Uuid::new_v4().to_string();

        tokio::spawn(async move {
            crate::events::emit_safe(
                status_emitter.as_ref(),
                crate::events::ProcessEvent::new(
                    crate::events::ProcessEventKind::VoiceSessionStart,
                    request_id.clone(),
                    model.clone(),
                ),
            )
            .await;
            let _ = event_tx
                .send(VoiceEvent::SessionCreated { session_id: None })
                .await;
            let mut decode = DecodeState::new(sample_rate);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => {
                        let _ = writer.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                        let _ = event_tx.send(VoiceEvent::Closed).await;
                        crate::events::emit_safe(
                            status_emitter.as_ref(),
                            crate::events::ProcessEvent::new(
                                crate::events::ProcessEventKind::VoiceSessionEnd,
                                request_id.clone(),
                                model.clone(),
                            ),
                        ).await;
                        break;
                    }
                    command = command_rx.recv() => {
                        match command {
                            Some(VoiceClientEvent::Close) | None => {
                                let _ = writer.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                                let _ = event_tx.send(VoiceEvent::Closed).await;
                                crate::events::emit_safe(
                                    status_emitter.as_ref(),
                                    crate::events::ProcessEvent::new(
                                        crate::events::ProcessEventKind::VoiceSessionEnd,
                                        request_id.clone(),
                                        model.clone(),
                                    ),
                                ).await;
                                break;
                            }
                            Some(command) => {
                                if let Err(error) = send_client_event(
                                    &mut writer,
                                    provider.as_ref(),
                                    &decode.playback,
                                    command,
                                )
                                .await
                                {
                                    let _ = event_tx.send(VoiceEvent::Error { message: error.to_string() }).await;
                                    break;
                                }
                            }
                        }
                    }
                    message = reader.next() => {
                        match message {
                            Some(Ok(message)) => {
                                if let Err(error) =
                                    forward_server_message(message, &event_tx, &mut decode).await
                                {
                                    let _ = event_tx.send(VoiceEvent::Error { message: error.to_string() }).await;
                                    break;
                                }
                            }
                            Some(Err(error)) => {
                                let _ = event_tx.send(VoiceEvent::Error { message: error.to_string() }).await;
                                break;
                            }
                            None => {
                                let _ = event_tx.send(VoiceEvent::Closed).await;
                                crate::events::emit_safe(
                                    status_emitter.as_ref(),
                                    crate::events::ProcessEvent::new(
                                        crate::events::ProcessEventKind::VoiceSessionEnd,
                                        request_id.clone(),
                                        model.clone(),
                                    ),
                                ).await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok((
            Self {
                commands: command_tx,
            },
            VoiceEvents { receiver: event_rx },
        ))
    }

    pub async fn update(&self, update: Value) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::Update(update)).await
    }

    pub async fn send_audio(&self, audio: impl Into<Bytes>) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::Audio(audio.into())).await
    }

    pub async fn send_text(&self, text: impl Into<String>) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::Text(text.into())).await
    }

    pub async fn interrupt(&self) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::Interrupt).await
    }

    pub async fn submit_tool_result(
        &self,
        call_id: impl Into<String>,
        output: Value,
    ) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::ToolResult {
            call_id: call_id.into(),
            output,
        })
        .await
    }

    pub async fn create_response(&self) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::CreateResponse).await
    }

    pub async fn close(&self) -> Result<(), VoiceError> {
        self.send(VoiceClientEvent::Close).await
    }

    async fn send(&self, event: VoiceClientEvent) -> Result<(), VoiceError> {
        self.commands
            .send(event)
            .await
            .map_err(|_| VoiceError::Closed)
    }
}

async fn send_client_event<W>(
    writer: &mut W,
    provider: &dyn VoiceProvider,
    playback: &PlaybackState,
    event: VoiceClientEvent,
) -> Result<(), VoiceError>
where
    W: futures_util::Sink<tokio_tungstenite::tungstenite::Message> + Unpin,
    W::Error: Into<tokio_tungstenite::tungstenite::Error>,
{
    use tokio_tungstenite::tungstenite::Message;
    let text = |value: Value| Message::Text(value.to_string().into());
    let messages = match event {
        VoiceClientEvent::Update(value) => vec![text(value)],
        VoiceClientEvent::Audio(audio) => vec![provider.encode_audio(audio)],
        VoiceClientEvent::Text(input) => vec![
            text(json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "message",
                    "role": "user",
                    "content": [{"type": "input_text", "text": input}]
                }
            })),
            text(json!({"type": "response.create"})),
        ],
        VoiceClientEvent::Interrupt => provider.interrupt(playback).into_iter().map(text).collect(),
        VoiceClientEvent::CreateResponse => vec![text(json!({"type": "response.create"}))],
        VoiceClientEvent::ToolResult { call_id, output } => vec![text(json!({
            "type": "conversation.item.create",
            "item": {
                "type": "function_call_output",
                "call_id": call_id,
                "output": output.to_string()
            }
        }))],
        VoiceClientEvent::Close => vec![Message::Close(None)],
    };
    for message in messages {
        writer.send(message).await.map_err(Into::into)?;
    }
    Ok(())
}

struct DecodeState {
    saw_binary_audio: bool,
    sample_rate: u32,
    audio_bytes: u64,
    playback: PlaybackState,
}

impl DecodeState {
    fn new(sample_rate: u32) -> Self {
        Self {
            saw_binary_audio: false,
            sample_rate,
            audio_bytes: 0,
            playback: PlaybackState::default(),
        }
    }

    /// Count delivered PCM16 mono audio: two bytes per sample.
    fn add_audio(&mut self, bytes: usize) {
        self.audio_bytes = self.audio_bytes.saturating_add(bytes as u64);
        let bytes_per_second = u64::from(self.sample_rate.max(1)) * 2;
        self.playback.audio_ms = self.audio_bytes.saturating_mul(1000) / bytes_per_second;
    }

    fn reset_playback(&mut self) {
        self.audio_bytes = 0;
        self.playback = PlaybackState::default();
    }
}

async fn forward_server_message(
    message: tokio_tungstenite::tungstenite::Message,
    events: &mpsc::Sender<VoiceEvent>,
    decode: &mut DecodeState,
) -> Result<(), VoiceError> {
    use tokio_tungstenite::tungstenite::Message;
    match message {
        Message::Binary(bytes) => {
            decode.saw_binary_audio = true;
            decode.add_audio(bytes.len());
            events
                .send(VoiceEvent::OutputAudio(bytes))
                .await
                .map_err(|_| VoiceError::Closed)?;
        }
        Message::Text(text) => {
            let payload: Value = serde_json::from_str(&text)
                .map_err(|error| VoiceError::Protocol(error.to_string()))?;
            if let Some(event) = normalize_server_event(&payload, decode)? {
                events.send(event).await.map_err(|_| VoiceError::Closed)?;
            }
        }
        Message::Close(_) => {
            events
                .send(VoiceEvent::Closed)
                .await
                .map_err(|_| VoiceError::Closed)?;
        }
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
    }
    Ok(())
}

fn normalize_server_event(
    payload: &Value,
    decode: &mut DecodeState,
) -> Result<Option<VoiceEvent>, VoiceError> {
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| VoiceError::Protocol("server event has no type".to_string()))?;
    if matches!(event_type, "response.created" | "response.done") {
        decode.reset_playback();
    }
    let text = |key: &str| {
        payload
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let event = match event_type {
        "conversation.created" => Some(VoiceEvent::SessionCreated {
            session_id: payload
                .pointer("/conversation/id")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "session.updated" => Some(VoiceEvent::SessionUpdated),
        "input_audio_buffer.speech_started" => Some(VoiceEvent::SpeechStarted),
        "input_audio_buffer.speech_stopped" => Some(VoiceEvent::SpeechStopped),
        "conversation.item.input_audio_transcription.completed" => {
            Some(VoiceEvent::InputTranscript {
                text: text("transcript"),
                final_: true,
            })
        }
        "response.output_audio_transcript.delta" => {
            let delta = text("delta");
            if delta.is_empty() {
                None
            } else {
                Some(VoiceEvent::OutputTranscript {
                    text: delta,
                    final_: false,
                })
            }
        }
        // OpenAI-compat aliases + *.done payloads repeat the canonical stream.
        "response.audio_transcript.delta"
        | "response.output_audio_transcript.done"
        | "response.audio_transcript.done" => None,
        "response.output_audio.delta" | "response.audio.delta" => {
            if decode.saw_binary_audio {
                None
            } else {
                let encoded = text("delta");
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|error| VoiceError::Protocol(error.to_string()))?;
                if let Some(item_id) = payload.get("item_id").and_then(Value::as_str) {
                    decode.playback.item_id = Some(item_id.to_string());
                }
                decode.add_audio(decoded.len());
                Some(VoiceEvent::OutputAudio(Bytes::from(decoded)))
            }
        }
        "response.function_call_arguments.done" => Some(VoiceEvent::FunctionCall {
            call_id: text("call_id"),
            name: text("name"),
            arguments: text("arguments"),
        }),
        "response.done" => Some(VoiceEvent::ResponseDone),
        "response.cancelled" => Some(VoiceEvent::Interrupted),
        "error" => Some(VoiceEvent::Error {
            message: payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("provider error")
                .to_string(),
        }),
        _ => Some(VoiceEvent::Provider {
            event_type: event_type.to_string(),
            payload: payload.clone(),
        }),
    };
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::accept_async;
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn connects_and_forwards_binary_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let _ = socket.next().await;
            socket
                .send(Message::Binary(Bytes::from_static(b"pcm")))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({
                        "type": "conversation.created",
                        "conversation": {"id": "conversation-test"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let mut credentials = crate::providers::ProviderCredentials::new();
        credentials.insert_key(crate::providers::ProviderId::Xai, "test-key");
        let mut config = VoiceConfig::xai(credentials);
        config.base_url = Some(format!("ws://{address}"));
        let (session, mut events) = VoiceSession::connect(config).await.unwrap();

        let mut audio = None;
        for _ in 0..3 {
            if let Some(VoiceEvent::OutputAudio(value)) =
                timeout(Duration::from_secs(1), events.recv())
                    .await
                    .unwrap()
            {
                audio = Some(value);
                break;
            }
        }
        assert_eq!(audio, Some(Bytes::from_static(b"pcm")));
        let _ = session.close().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn openai_session_sends_ga_update_and_base64_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let mut frames = Vec::new();
            for _ in 0..2 {
                match socket.next().await {
                    Some(Ok(Message::Text(text))) => {
                        frames.push(serde_json::from_str::<Value>(&text).unwrap());
                    }
                    other => panic!("expected a text frame, got {other:?}"),
                }
            }
            frames
        });

        let mut credentials = crate::providers::ProviderCredentials::new();
        credentials.insert_key(crate::providers::ProviderId::OpenAi, "test-key");
        let mut config = VoiceConfig::openai(credentials, "gpt-realtime");
        config.base_url = Some(format!("ws://{address}"));
        let (session, _events) = VoiceSession::connect(config).await.unwrap();
        session
            .send_audio(Bytes::from_static(b"pcm"))
            .await
            .unwrap();

        let frames = timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frames[0]["type"], "session.update");
        assert_eq!(frames[0]["session"]["type"], "realtime");
        assert_eq!(frames[1]["type"], "input_audio_buffer.append");
        assert_eq!(frames[1]["audio"], "cGNt");
        let _ = session.close().await;
    }

    fn normalize(payload: Value) -> Option<VoiceEvent> {
        normalize_server_event(&payload, &mut DecodeState::new(24_000)).unwrap()
    }

    #[test]
    fn normalizes_function_calls_and_base64_audio() {
        let function = normalize(json!({
            "type": "response.function_call_arguments.done",
            "call_id": "call-1",
            "name": "search",
            "arguments": "{\"q\":\"rust\"}"
        }));
        assert!(matches!(
            function,
            Some(VoiceEvent::FunctionCall { call_id, name, .. })
                if call_id == "call-1" && name == "search"
        ));

        let audio = normalize(json!({
            "type": "response.output_audio.delta",
            "delta": "cGNt"
        }));
        assert!(
            matches!(audio, Some(VoiceEvent::OutputAudio(value)) if value == Bytes::from_static(b"pcm"))
        );

        let delta = normalize(json!({
            "type": "response.output_audio_transcript.delta",
            "delta": "Four."
        }));
        assert!(matches!(
            delta,
            Some(VoiceEvent::OutputTranscript { text, final_: false }) if text == "Four."
        ));
        let done = normalize(json!({
            "type": "response.output_audio_transcript.done",
            "transcript": "Four."
        }));
        assert!(done.is_none());
    }

    #[test]
    fn drops_compat_aliases_and_json_audio_after_binary() {
        assert!(
            normalize(json!({
                "type": "response.audio_transcript.delta",
                "delta": "Four."
            }))
            .is_none()
        );

        let mut decode = DecodeState::new(24_000);
        decode.saw_binary_audio = true;
        let skipped = normalize_server_event(
            &json!({
                "type": "response.output_audio.delta",
                "delta": "cGNt"
            }),
            &mut decode,
        )
        .unwrap();
        assert!(skipped.is_none());
    }

    #[test]
    fn tracks_delivered_audio_for_truncate() {
        let mut decode = DecodeState::new(24_000);
        // 4800 bytes of PCM16 at 24 kHz is 100 ms.
        let chunk = base64::engine::general_purpose::STANDARD.encode(vec![0u8; 4800]);
        for _ in 0..2 {
            normalize_server_event(
                &json!({
                    "type": "response.output_audio.delta",
                    "item_id": "item-7",
                    "delta": chunk
                }),
                &mut decode,
            )
            .unwrap();
        }
        assert_eq!(
            decode.playback,
            PlaybackState {
                item_id: Some("item-7".into()),
                audio_ms: 200
            }
        );

        normalize_server_event(&json!({"type": "response.done"}), &mut decode).unwrap();
        assert_eq!(decode.playback, PlaybackState::default());
    }
}
