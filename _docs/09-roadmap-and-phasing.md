# Roadmap and phasing

## Principles

- Ship a narrow slice, then widen it.
- Record an intentional behavior change in these notes.

## Phase 1: HTTP, retries, streaming

Shipped.

- `HttpClient` with rustls
- `SseParser`
- Distinct GET and POST retry rules
- `governor` rate limits
- Wiremock tests

## Phase 2: Tools and Python

Shipped.

- `Tool` and `ToolRegistry`
- `complete_with_tools`
- Offline harness
- `superglue-py` on PyO3, CPython 3.12+ including 3.14t
- `complete` and `stream` with host callbacks

## Phase 3: Provider types and protobuf

Shipped.

- OpenAI-shaped chat types, including multipart content
- `proto/superglue.proto` and `prost` generation in `build.rs`
- Streaming usage on the final SSE chunk

## Phase 4: gRPC, audit, and more bindings

Shipped, except the workflow engine.

- tonic RPCs: `Complete`, `Stream`, `CompleteResponses`, `StreamResponses`
- `RunStore`, `RunRecorder`, `resume_chat`, `resume_response`
- `scripts/generate-proto-clients.sh` for Python and JavaScript stubs
- Node.js binding (`superglue-js`) and Kotlin JNI binding (`superglue-kt`)
- Workflow engine: planned

## Phase 5: Hardening and context control

Shipped.

- `ScrubMode`: `Redact`, `Hash`, `Allow`
- OTLP export (`otlp`) and Prometheus (`prometheus`)
- `secrecy` API keys with `zeroize` on drop
- Batch scheduling and parallel tools in one round
- `FailFast`, `Skip`, and `Retry`
- `CancellationToken` in chat and batch
- Throttled downloads
- Context optimization and code mode (`code`)
- Embeddings, files, images, fallback, and System One

## Phase 6: Realtime voice

Shipped for xAI. Other adapters are planned.

- `realtime` feature and `VoiceSession`
- xAI Speech to Speech adapter
- xAI file transcription (`grok-voice-transcribe-2.0` on `POST /v1/stt`)
- Rust example `38_file_transcription`
- Binary audio, transcripts, interruptions, and function-call events
- Cancel closes the provider WebSocket
- Rust example `36_realtime_voice`
- OpenAI Realtime adapter: planned
- Voice methods on the language bindings: planned

## Phase 7: LLM gateway

Shipped.

- OpenAI-compatible HTTP proxy (`gateway`)
- Virtual keys, users, profiles, budgets, and usage in SQLite
- Embeddings, Responses, System One, and file transcription routes
- Self-hosted proxy providers
- Admin UI in `gateway-admin/`
- Optional S3 capture (`capture`)

## Still planned

- Workflow engine and protobuf workflow transitions
- OpenAI Realtime adapter and voice APIs on the bindings

Context flags are in [Context optimization](11-context-optimization.md).

## Cross-cutting work

- Keep wiremock coverage on streaming, tools, and provider adapters.
- Update this directory when a phase changes status.
