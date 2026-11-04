package com.superglue.kt;

/**
 * Receives realtime voice events on a native background thread.
 *
 * <p>{@code eventJson} holds the event fields. {@code audio} holds PCM bytes for an
 * {@code output_audio} event and is {@code null} otherwise.
 */
@FunctionalInterface
public interface VoiceEventCallback {
    void onEvent(String eventJson, byte[] audio);
}
