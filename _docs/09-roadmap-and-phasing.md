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

Shipped.

- tonic RPCs: `Complete`, `Stream`, `CompleteResponses`, `StreamResponses`
- `AuditStore`, `MemoryRunStore`, `RunRecorder`, `resume_chat`, `resume_response`
- `WorkflowTransition` lines and `JsonlRunStore`, so a partial unary run can resume after a crash
- `scripts/generate-proto-clients.sh` for Python and JavaScript stubs
- Node.js binding (`superglue-js`) and Kotlin JNI binding (`superglue-kt`)
- A general workflow engine is out of scope. See [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md).

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

Shipped.

- `realtime` feature and `VoiceSession`
- `VoiceProvider` owns the session payload, the audio encoding, and the interrupt frames
- xAI Speech to Speech adapter
- OpenAI Realtime adapter: GA `session.update`, base64 audio, `response.cancel`, and `conversation.item.truncate`
- xAI file transcription (`grok-voice-transcribe-2.0` on `POST /v1/stt`)
- Rust example `38_file_transcription`
- Binary audio, transcripts, interruptions, and function-call events
- Cancel closes the provider WebSocket
- Rust example `36_realtime_voice`
- Voice sessions on the Python, Node.js, and Kotlin bindings

## Phase 7: LLM gateway

Shipped.

- OpenAI-compatible HTTP proxy (`gateway`)
- Virtual keys, users, profiles, budgets, and usage in SQLite
- Budget holds for in-flight requests (schema v5)
- Embeddings, Responses, System One, and file transcription routes
- Self-hosted proxy providers
- Admin UI in `gateway-admin/`
- Optional S3 capture (`capture`)

## Still planned

No phase work is open.

Context flags are in [Context optimization](11-context-optimization.md).

## Cross-cutting work

- Keep wiremock coverage on streaming, tools, and provider adapters.
- Update this directory when a phase changes status.
