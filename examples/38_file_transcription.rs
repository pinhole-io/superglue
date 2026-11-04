//! File transcription for xAI, OpenAI, or Groq.
//!
//! Run with:
//! `XAI_API_KEY=... cargo run --example 38_file_transcription`
//!
//! The default file is `examples/sample.mp3`.
//! Pass another path to transcribe that file instead.
//!
//! `SUPERGLUE_STT_PROVIDER` selects `xai` (default), `openai`, or `groq`.
//! xAI uses `grok-voice-transcribe-2.0`. OpenAI uses `whisper-1`.
//! Groq uses `whisper-large-v3`.

use std::env;
use std::path::PathBuf;
use std::str::FromStr;

use superglue::audio::{self, default_transcription_model};
use superglue::http::{ClientConfig, HttpClient};
use superglue::providers::{ProviderCredentials, ProviderId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sample.mp3"));
    let provider_name = env::var("SUPERGLUE_STT_PROVIDER").unwrap_or_else(|_| "xai".to_string());
    let provider = ProviderId::from_str(&provider_name)?;
    if !audio::supports_transcription(provider) {
        return Err(format!("file transcription is not supported for {provider}").into());
    }
    let credentials = ProviderCredentials::from_env();
    if !credentials.has_key(provider) {
        println!(
            "Skip file transcription example (set {})",
            provider.env_var_for_key()
        );
        return Ok(());
    }
    let model = default_transcription_model(provider).unwrap_or("unknown");
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("audio.wav");
    let bytes = std::fs::read(&path)?;
    let http = HttpClient::new(ClientConfig::default())?;
    let transcript = audio::transcribe(&http, &credentials, provider, filename, bytes).await?;

    println!("{provider} model {model}");
    println!("{}", transcript.text);
    for word in &transcript.words {
        println!("{:.2}-{:.2} {}", word.start, word.end, word.text);
    }
    Ok(())
}
