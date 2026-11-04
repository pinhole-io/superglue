//! Provider-agnostic realtime voice sessions.
//!
//! This module is feature-gated because the normal SuperGlue transport remains
//! HTTP and SSE. Adapters exist for xAI Speech to Speech and OpenAI Realtime.

mod binding;
mod error;
mod events;
mod openai;
mod provider;
mod session;
mod xai;

pub use binding::{voice_config_from_json, voice_event_audio, voice_event_json};
pub use error::VoiceError;
pub use events::{VoiceClientEvent, VoiceEvent};
pub use provider::{PlaybackState, VoiceProvider, resolve_voice_provider};
pub use session::{VoiceConfig, VoiceEvents, VoiceSession};
