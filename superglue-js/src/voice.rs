//! Realtime voice sessions for Node.js.
//!
//! Events reach JS through a threadsafe function called from a blocking
//! thread, the same hop `Client.stream` uses for tokens.

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use serde_json::Value;
use superglue::providers::ProviderCredentials;
use superglue::realtime::{
    VoiceError, VoiceSession as SgVoiceSession, voice_config_from_json, voice_event_audio,
    voice_event_json,
};

/// One realtime event. `audio` holds PCM bytes for an `output_audio` event.
#[napi(object)]
pub struct VoiceEventJs {
    #[napi(ts_type = "Record<string, unknown>")]
    pub event: Value,
    pub audio: Option<Buffer>,
}

/// A connected realtime voice session. Create it with `Client.voiceConnect`.
#[napi]
pub struct VoiceSession {
    inner: SgVoiceSession,
}

pub(crate) async fn connect(
    credentials: ProviderCredentials,
    config: Value,
    on_event: ThreadsafeFunction<VoiceEventJs, ()>,
) -> Result<VoiceSession> {
    let config = voice_config_from_json(&config.to_string(), credentials).map_err(voice_error)?;
    let (session, mut events) = SgVoiceSession::connect(config).await.map_err(voice_error)?;
    tokio::task::spawn_blocking(move || {
        while let Some(event) = events.blocking_recv() {
            let payload = VoiceEventJs {
                event: voice_event_json(&event),
                audio: voice_event_audio(&event).map(|audio| Buffer::from(audio.to_vec())),
            };
            let _ = on_event.call(Ok(payload), ThreadsafeFunctionCallMode::Blocking);
        }
    });
    Ok(VoiceSession { inner: session })
}

fn voice_error(error: VoiceError) -> Error {
    let status = match error {
        VoiceError::Config(_) | VoiceError::UnsupportedProvider => Status::InvalidArg,
        _ => Status::GenericFailure,
    };
    Error::new(status, error.to_string())
}

#[napi]
impl VoiceSession {
    /// Send a user text message and request a response.
    #[napi]
    pub async fn send_text(&self, text: String) -> Result<()> {
        self.inner.send_text(text).await.map_err(voice_error)
    }

    /// Send one frame of PCM16 mono audio at the session sample rate.
    #[napi]
    pub async fn send_audio(&self, audio: Buffer) -> Result<()> {
        self.inner
            .send_audio(audio.to_vec())
            .await
            .map_err(voice_error)
    }

    /// Stop the current response.
    #[napi]
    pub async fn interrupt(&self) -> Result<()> {
        self.inner.interrupt().await.map_err(voice_error)
    }

    /// Return a tool result for a `function_call` event and request the next response.
    #[napi]
    pub async fn submit_tool_result(
        &self,
        call_id: String,
        #[napi(ts_arg_type = "unknown")] output: Value,
    ) -> Result<()> {
        self.inner
            .submit_tool_result(call_id, output)
            .await
            .map_err(voice_error)?;
        self.inner.create_response().await.map_err(voice_error)
    }

    /// Close the provider connection. A closed session rejects later commands.
    #[napi]
    pub async fn close(&self) -> Result<()> {
        self.inner.close().await.map_err(voice_error)
    }
}
