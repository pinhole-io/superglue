# Architecture

## Summary

The Rust core owns provider I/O, SSE, retries, rate limits, the tool loop, and optional voice sessions.
A binding registers tools, supplies configuration, and returns results to the host.
The gateway is an optional HTTP front door. It uses the same providers.

## Module map

| Module | Duty |
| --- | --- |
| `http` | `reqwest` client, retry policy, per-client QPS, SSE parser, throttled download |
| `openai` | OpenAI-shaped chat request and response types |
| `providers` | `provider:model` parsing, credentials, Anthropic and Bedrock adapters |
| `chat` | Tool loop, streaming tool rounds, `Conversation` |
| `responses` | Responses API tool loop and gateway proxy helpers |
| `client` | `Client`, `ClientBuilder`, `ClientConversation`, `CallOptions` |
| `tools` | `Tool` trait, registry, harness, error policy, code mode |
| `context` | Dynamic routing, round condensing, AAAK, history summary |
| `batch` | Concurrent completions |
| `agents` | Persona, goals, constraints, `AgentEngine` |
| `hooks` | Pipeline hooks that can change content |
| `guardrails` | Checks that can block content |
| `events` | Observation-only `ProcessEvent` records |
| `fallback` | Next model after retries fail on the current model |
| `files` | Inline file bytes and provider file upload |
| `images` | Image parts and lazy image refs |
| `embeddings` | `/v1/embeddings`, plus Bedrock `InvokeModel` |
| `audio` | File transcription for OpenAI and Groq |
| `systemone` | TypeSafe System One evaluation |
| `usage` | Token breakdown helpers |
| `costing` | Static USD rates and billed-cost lookup |
| `audit` | In-memory `RunStore`, `RunRecorder`, resume helpers |
| `telemetry` | Tracing, scrub modes, OpenInference attributes, metrics |
| `cancel` | `CancellationToken` re-export |
| `proto` | Types generated from `proto/superglue.proto` |
| `content_quality` | Heuristics for low-signal web text |
| `mcp` | MCP clients. Feature `mcp`. |
| `realtime` | Duplex voice sessions. Feature `realtime`. xAI adapter. |
| `grpc` | tonic server. Feature `grpc`. |
| `gateway` | OpenAI-compatible HTTP proxy. Feature `gateway`. |

`Client::conversation` returns a `ClientConversation`.
`push_user` appends text.
`push_user_message` appends a user message that can contain file and image parts. The method sets the role to `user`.
`complete` runs one turn and stores the returned history.
`stream` does the same for tokens. On OpenAI, that stream uses the Responses API so a reasoning summary can arrive while tools stay enabled. Other chat providers use chat completions.

Python and JavaScript expose `push_user` for text.
They do not expose `push_user_message` yet.
Those bindings attach files with `message_with_file_bytes` and `complete_messages`.

## What the core implements

| Piece | Behavior |
| --- | --- |
| `HttpClient` | GET with full retries. POST JSON with POST-safe retries. Streaming POST returns bytes. |
| `RetryPolicy` | GET retries retryable statuses. POST retries transport errors and 429, 502, 503, 504. |
| `complete_with_tools` | Multi-round tool loop. One round runs handlers concurrently with `join_all`. Tool messages follow `tool_calls` order. |
| `stream_complete` | SSE deltas for one turn when no tools are registered |
| Streaming with tools | Same multi-round loop. Handlers can start as arguments complete. Final tool messages follow `tool_calls` order. |
| `OnToolError` | `FailFast` (default), `Skip`, or `Retry` with exponential backoff |
| `tools::harness` | Scripted tool plan with no live model |
| `VoiceSession` | Duplex WebSocket audio, transcripts, interruptions, and function-call events |
| `RunStore` | Bounded in-memory `proto::RunRecord` store |
| gRPC | `Complete`, `Stream`, `CompleteResponses`, `StreamResponses` |

## Bindings

| Binding | Status | Notes |
| --- | --- | --- |
| Rust `Client` | Shipped | Library and CLI in this crate |
| Python `superglue-py` | Shipped | PyO3. Public import name `superglue`. CPython 3.12+ wheels, including 3.14t. |
| Node.js `superglue-js` | Shipped | napi-rs. Prefer `superglue-js/facade`. |
| Kotlin `superglue-kt` | Shipped | JNI. Android ABIs `arm64-v8a` and `x86_64`. |
| gRPC sidecar | Shipped | `superglue serve` with `--features grpc` |

Python `register_tool` infers JSON Schema from the function. See [Tool boundary and async](03-tool-boundary-and-async.md).
JavaScript and Kotlin require an explicit JSON Schema.

The Python, Node, and Kotlin bindings compile the core with `code` and `mcp`.
Gateway, gRPC, OTLP, Prometheus, capture, and realtime stay off unless you enable them on the Rust crate or CLI.

## Surfaces

```text
Python / JavaScript / Kotlin / Rust / CLI
            │  JSON tools, native callbacks
            ▼
     SuperGlue (Rust, Tokio)
            │  HTTP, SSE, optional WebSocket
            ▼
     Model providers

Optional:  client  →  gateway (virtual keys, budgets)  →  providers
Optional:  gRPC client  →  tonic server  →  same core
```

## Python token delivery

`Python::attach` must not run from a Tokio poll on free-threaded CPython.

1. The stream callback sends each token on an `mpsc` channel.
2. A `spawn_blocking` task reads that channel and calls Python.
3. `block_on` returns after the stream and that task finish.

Tool callbacks use the same `spawn_blocking` bridge.
JavaScript uses the napi async runtime.
Kotlin uses JNI.

## See also

- [Multi-provider guide](../docs/multi-provider.md)
- [LLM gateway](12-llm-gateway.md)
- [Context optimization](11-context-optimization.md)
