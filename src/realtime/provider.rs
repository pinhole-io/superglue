//! Provider boundary for realtime sessions.

use async_trait::async_trait;
use bytes::Bytes;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use url::Url;

use super::error::VoiceError;
use super::session::VoiceConfig;

pub(crate) type VoiceWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Assistant audio the session has delivered for the current response.
///
/// `audio_ms` counts audio handed to the caller. Playback on the caller side can lag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaybackState {
    pub item_id: Option<String>,
    pub audio_ms: u64,
}

/// Wire differences between realtime providers.
#[async_trait]
pub trait VoiceProvider: Send + Sync {
    fn provider_id(&self) -> crate::providers::ProviderId;

    async fn connect(&self, config: &VoiceConfig) -> Result<VoiceWebSocket, VoiceError>;

    /// The first `session.update` sent after connect.
    fn session_update(&self, config: &VoiceConfig) -> Value;

    /// One frame of caller PCM audio.
    fn encode_audio(&self, audio: Bytes) -> Message;

    /// Events that stop the current response.
    fn interrupt(&self, playback: &PlaybackState) -> Vec<Value>;
}

pub fn resolve_voice_provider(
    provider: crate::providers::ProviderId,
) -> Result<Box<dyn VoiceProvider>, VoiceError> {
    match provider {
        crate::providers::ProviderId::Xai => Ok(Box::new(super::xai::XaiVoiceProvider)),
        crate::providers::ProviderId::OpenAi => Ok(Box::new(super::openai::OpenAiVoiceProvider)),
        _ => Err(VoiceError::UnsupportedProvider),
    }
}

/// Open `{base}/v1/realtime` with bearer auth from the provider credential.
pub(crate) async fn connect_bearer(
    provider: crate::providers::ProviderId,
    config: &VoiceConfig,
    query: &[(&str, &str)],
) -> Result<VoiceWebSocket, VoiceError> {
    let key = config.credentials.key_for(provider)?;
    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| config.credentials.base_url_for(provider));
    let base_url = base_url.trim_end_matches('/');
    let websocket_base_url = if let Some(value) = base_url.strip_prefix("https://") {
        format!("wss://{value}")
    } else if let Some(value) = base_url.strip_prefix("http://") {
        format!("ws://{value}")
    } else if base_url.starts_with("ws://") || base_url.starts_with("wss://") {
        base_url.to_owned()
    } else {
        return Err(VoiceError::Protocol(format!(
            "realtime base URL must use http(s) or ws(s), got {base_url}"
        )));
    };
    let mut url = Url::parse(&format!("{websocket_base_url}/v1/realtime"))
        .map_err(|error| VoiceError::Protocol(format!("invalid realtime URL: {error}")))?;
    {
        let mut pairs = url.query_pairs_mut();
        for (name, value) in query {
            pairs.append_pair(name, value);
        }
    }
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|error| VoiceError::Protocol(format!("invalid WebSocket request: {error}")))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", key.expose_secret())
            .parse()
            .map_err(|_| VoiceError::Protocol("invalid authorization header".to_string()))?,
    );
    request.headers_mut().insert(
        "User-Agent",
        format!("superglue/{}", env!("CARGO_PKG_VERSION"))
            .parse()
            .map_err(|error| VoiceError::Protocol(format!("invalid user agent header: {error}")))?,
    );
    // Harn also uses reqwest with aws-lc-rs. Explicitly selecting ring here
    // prevents rustls from panicking when both providers are linked.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (socket, _) = connect_async(request).await.map_err(|error| {
        VoiceError::Protocol(format!(
            "realtime WebSocket connection failed: {error} ({url})"
        ))
    })?;
    Ok(socket)
}

/// Function tools in the realtime `session.tools` shape both providers accept.
pub(crate) fn tool_definitions(tools: &[crate::tools::ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters_schema
                })
            })
            .collect(),
    )
}
