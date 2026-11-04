# superglue-kt (Kotlin / Android)

JNI bindings to the in-process Rust crate.
The feature surface matches [superglue-js](../superglue-js) and [superglue-py](../superglue-py).

## Layout

- `native/` is the Rust cdylib `libsuperglue_kt.so` (`superglue_kt`).
- `superglue-android/` is the Android library. Kotlin plus `SuperglueNativeJni`.

The native crate enables the `code` and `mcp` features of the Rust core.
Library `minSdk` is 24.

## Build the native library

You need Rust, the Android NDK, and [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk).

From `superglue-kt/`:

```bash
export ANDROID_NDK_HOME=/path/to/ndk
./gradlew :superglue-android:buildRustJni
./gradlew :superglue-android:assembleDebug
```

The wrapper is `superglue-kt/gradlew`.

When `ANDROID_NDK_HOME` or `android.ndkPath` is set, `preBuild` runs `buildRustJni`.
Without the NDK, build the shared library in an environment that has it, then reopen the project.

Host unit tests set `SUPERGLUE_FORCE_HOST_JNI=1` so they use host `cargo` instead of `cargo-ndk`.
CI does that. A JVM test must tolerate a missing `.so`.

## Install

```kotlin
dependencies {
    implementation("com.superglue.kt:superglue-android:<version>")
}
```

Replace `<version>` with the release you use.
The AAR contains `arm64-v8a` and `x86_64`.
Each GitHub release also stores `superglue-android-<version>.aar`.

A file dependency does not pull AndroidX or kotlinx.
Add those libraries yourself, or include the `:superglue-android` module from this repository.

Publish steps: [PACKAGES.md](../PACKAGES.md).

## Butler

Butler is a separate Android app.
It can include `:superglue-android` and call `com.superglue.kt`.
`com.butler.data.ai.SuperglueBridge` is the Butler probe used when the `.so` is absent on host tests.
That class is not in this repository.

## Examples

JUnit examples and the number map: [examples/README.md](examples/README.md).

## API contract

The JNI boundary follows `superglue-js/index.d.ts`.
Clients, tools, hooks, guardrails, streaming, batch, conversations, and the agent engine use the same names.
`SuperglueClient.voiceConnect(configJson, onEvent)` opens a realtime voice session.
`VoiceEventCallback` runs on a native thread. See [`../docs/multi-provider.md`](../docs/multi-provider.md#realtime-voice).
