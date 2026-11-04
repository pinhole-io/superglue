//! Realtime voice session JNI (`VoiceEventCallback`).
//!
//! Events reach the JVM on one attached OS thread per session, the same hop
//! `clientStream` uses for tokens. Each event runs in its own local frame, so a
//! long session does not fill the local reference table.

use std::future::Future;

use jni::JNIEnv;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::jlong;
use superglue::realtime::{
    VoiceError, VoiceEvent, VoiceSession, voice_config_from_json, voice_event_audio,
    voice_event_json,
};

use crate::handle::{KHandle, alloc, get_client, get_voice, remove};
use crate::jni_base::{ensure_jvm, jthrow, runtime};
use crate::jni_client::jstring_to_rust;

fn deliver(env: &mut JNIEnv, callback: &GlobalRef, event: &VoiceEvent) -> jni::errors::Result<()> {
    env.with_local_frame(4, |env| {
        let json = env.new_string(voice_event_json(event).to_string())?;
        let audio = match voice_event_audio(event) {
            Some(bytes) => JObject::from(env.byte_array_from_slice(bytes)?),
            None => JObject::null(),
        };
        env.call_method(
            callback,
            "onEvent",
            "(Ljava/lang/String;[B)V",
            &[JValue::Object(&json), JValue::Object(&audio)],
        )?;
        Ok(())
    })
}

/// Run one session command on the shared runtime and map the error for Java.
fn command<F, Fut>(env: &mut JNIEnv, handle: jlong, send: F)
where
    F: FnOnce(VoiceSession) -> Fut,
    Fut: Future<Output = Result<(), VoiceError>>,
{
    let session = match get_voice(handle as u64) {
        Ok(session) => session,
        Err(error) => return jthrow(env, &error),
    };
    if let Err(error) = runtime().block_on(send(session)) {
        jthrow(env, &error.to_string());
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_clientVoiceConnect(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    config_json: JString,
    on_event: JObject,
) -> jlong {
    let jvm = ensure_jvm(&mut env);
    let state = match get_client(handle as u64) {
        Ok(state) => state,
        Err(error) => {
            jthrow(&mut env, &error);
            return 0;
        }
    };
    let callback = match env.new_global_ref(&on_event) {
        Ok(callback) => callback,
        Err(error) => {
            jthrow(&mut env, &error.to_string());
            return 0;
        }
    };
    let config_json = jstring_to_rust(&mut env, &config_json).unwrap_or_default();
    let credentials = superglue::chat::credentials_for(&state.options);
    let config = match voice_config_from_json(&config_json, credentials) {
        Ok(config) => config,
        Err(error) => {
            jthrow(&mut env, &error.to_string());
            return 0;
        }
    };
    let (session, mut events) = match runtime().block_on(VoiceSession::connect(config)) {
        Ok(pair) => pair,
        Err(error) => {
            jthrow(&mut env, &error.to_string());
            return 0;
        }
    };
    std::thread::spawn(move || {
        let Ok(mut env) = jvm.attach_current_thread() else {
            return;
        };
        while let Some(event) = events.blocking_recv() {
            if deliver(&mut env, &callback, &event).is_err()
                && env.exception_check().unwrap_or(false)
            {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
        }
    });
    alloc(KHandle::Voice(session)) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_voiceSendText(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    text: JString,
) {
    let text = jstring_to_rust(&mut env, &text).unwrap_or_default();
    command(&mut env, handle, |session| async move {
        session.send_text(text).await
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_voiceSendAudio(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    audio: JByteArray,
) {
    let audio = match env.convert_byte_array(&audio) {
        Ok(audio) => audio,
        Err(error) => return jthrow(&mut env, &error.to_string()),
    };
    command(&mut env, handle, |session| async move {
        session.send_audio(audio).await
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_voiceInterrupt(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) {
    command(&mut env, handle, |session| async move {
        session.interrupt().await
    });
}

/// Return a tool result and request the next response.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_voiceSubmitToolResult(
    mut env: JNIEnv,
    _cl: JClass,
    handle: jlong,
    call_id: JString,
    output_json: JString,
) {
    let call_id = jstring_to_rust(&mut env, &call_id).unwrap_or_default();
    let output_json = jstring_to_rust(&mut env, &output_json).unwrap_or_default();
    let output = match serde_json::from_str(&output_json) {
        Ok(output) => output,
        Err(error) => return jthrow(&mut env, &format!("tool output is not JSON: {error}")),
    };
    command(&mut env, handle, |session| async move {
        session.submit_tool_result(call_id, output).await?;
        session.create_response().await
    });
}

/// Close the session when it is still open, then free the handle.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_superglue_kt_SuperglueNativeJni_voiceDestroy(
    _env: JNIEnv,
    _cl: JClass,
    handle: jlong,
) {
    if let Ok(session) = get_voice(handle as u64) {
        let _ = runtime().block_on(session.close());
    }
    remove(handle as u64);
}
