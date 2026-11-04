//! Minimal realtime voice client.
//!
//! Run with xAI:
//! `XAI_API_KEY=... cargo run --features realtime --example 36_realtime_voice`
//!
//! Run with OpenAI Realtime:
//! `SUPERGLUE_VOICE_PROVIDER=openai OPENAI_API_KEY=... cargo run --features realtime --example 36_realtime_voice`
//!
//! `OPENAI_REALTIME_MODEL` overrides the OpenAI model `gpt-realtime`.
//! Type a line to send it. Type `/quit` to stop.

use std::sync::Arc;

use bytes::Bytes;
use superglue::providers::{ProviderCredentials, ProviderId};
use superglue::realtime::{VoiceConfig, VoiceEvent, VoiceSession};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn voice_config() -> Result<Option<VoiceConfig>, Box<dyn std::error::Error>> {
    let credentials = ProviderCredentials::from_env();
    let provider = std::env::var("SUPERGLUE_VOICE_PROVIDER").unwrap_or_else(|_| "xai".into());
    match provider.as_str() {
        "xai" => {
            if !credentials.has_key(ProviderId::Xai) {
                println!(
                    "Skip realtime voice example (set XAI_API_KEY, or SUPERGLUE_VOICE_PROVIDER=openai with OPENAI_API_KEY)"
                );
                return Ok(None);
            }
            Ok(Some(VoiceConfig::xai(credentials)))
        }
        "openai" => {
            if !credentials.has_key(ProviderId::OpenAi) {
                println!("Skip realtime voice example (set OPENAI_API_KEY)");
                return Ok(None);
            }
            let model =
                std::env::var("OPENAI_REALTIME_MODEL").unwrap_or_else(|_| "gpt-realtime".into());
            Ok(Some(VoiceConfig::openai(credentials, model)))
        }
        other => Err(format!("SUPERGLUE_VOICE_PROVIDER must be xai or openai, not {other}").into()),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(config) = voice_config()? else {
        return Ok(());
    };
    let cancel = config.cancel.clone();
    let (session, mut events) = VoiceSession::connect(config).await?;

    let reader = BufReader::new(tokio::io::stdin());
    let mut lines = reader.lines();
    let sender = Arc::new(session);
    let input_sender = Arc::clone(&sender);
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if line == "/quit" {
                cancel.cancel();
                break;
            }
            if let Err(error) = input_sender.send_text(line).await {
                eprintln!("send error: {error}");
                break;
            }
        }
    });

    while let Some(event) = events.recv().await {
        match event {
            VoiceEvent::OutputAudio(audio) => write_audio(audio).await?,
            VoiceEvent::InputTranscript { text, final_ } => {
                println!("you{}: {text}", if final_ { "" } else { "..." });
            }
            VoiceEvent::OutputTranscript { text, final_ } => {
                println!("assistant{}: {text}", if final_ { "" } else { "..." });
            }
            VoiceEvent::FunctionCall {
                call_id,
                name,
                arguments,
            } => println!("tool call {name} ({call_id}): {arguments}"),
            VoiceEvent::Error { message } => eprintln!("voice error: {message}"),
            VoiceEvent::Closed => break,
            _ => {}
        }
    }
    Ok(())
}

async fn write_audio(audio: Bytes) -> Result<(), std::io::Error> {
    // The example keeps the transport visible without choosing a platform
    // audio backend. Pipe stdout to a PCM player when testing.
    tokio::io::stdout().write_all(&audio).await
}
