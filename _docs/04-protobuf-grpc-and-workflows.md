# Protobuf, gRPC, and workflows

## Status

The protobuf schema is shipped.
The gRPC server is shipped behind the `grpc` feature.
Audit resume helpers are shipped.
The workflow engine is planned.

JSON stays the format for tool arguments and tool results.
See [Tool boundary and async](03-tool-boundary-and-async.md).

## Schema

The source of truth is [`proto/superglue.proto`](../proto/superglue.proto).
`build.rs` generates Rust with `prost`.
Do not copy field lists into this note. Edit the `.proto` file, then regenerate stubs.

Messages:

| Message | Role |
| --- | --- |
| `ToolSpec`, `FunctionCall`, `ToolCall` | Tool metadata and one model tool call |
| `ChatMessage`, `ChatOptions` | Chat transcript and sampling options |
| `CompletionRequest`, `CompletionOutcome`, `Usage` | Unary chat RPC |
| `ResponsesRequest`, `ResponsesOutcome` | Responses API RPC |
| `HookEvent`, `RunRecord` | Audit trail |
| `ProcessEvent`, `ProcessEventKind` | Status events |
| `StreamChunk` | Server-streaming token |

Service `SuperglueService`:

| RPC | Shape | Core path |
| --- | --- | --- |
| `Complete` | Unary | `chat::complete_with_tools` |
| `Stream` | Server stream | `chat::stream_complete` |
| `CompleteResponses` | Unary | `responses::complete_with_tools` |
| `StreamResponses` | Server stream | `responses::stream_complete_with_tools` |

`From<proto::ChatOptions> for ChatOptions` is the bridge into the Rust chat types.
`CompletionOutcome` carries `proto::Usage` through the pipeline and the bindings.

## Generate language stubs

```bash
bash scripts/generate-proto-clients.sh
```

The script writes Python stubs with `grpcio-tools` and a JavaScript loader under `superglue-js/generated/`.
CI checks that the committed Python stubs match the schema.
The Rust tonic client builds with `--features grpc`.

## Run the server

Build with the feature, then start the sidecar:

```bash
cargo build --features grpc --release
export OPENAI_API_KEY="sk-..."
./target/release/superglue serve --addr 127.0.0.1:50051
```

`--api-key` overrides the environment variable.
`--base-url` defaults to `https://api.openai.com`.
`--model` defaults to `gpt-5.4-nano-2026-03-17-mini`.
Bind `0.0.0.0` only when a firewall or reverse proxy owns the public edge.

The `serve` command builds an empty tool registry. Host tools stay on the in-process bindings.

## Deployment modes

| Mode | Status | Notes |
| --- | --- | --- |
| In-process Rust | Shipped | Depend on this crate |
| PyO3, napi-rs, JNI | Shipped | `superglue-py`, `superglue-js`, `superglue-kt` |
| gRPC sidecar | Shipped | `--features grpc` |

## Audit and workflows

Shipped today:

- `RunStore` and `RunRecorder`
- `ClientBuilder::audit`
- `audit::resume_chat` and `audit::resume_response`
- `export_record_json` and `import_record_json` for metadata replay files

The store is in memory and bounded.
Disk or an external store is a later choice. See [Security and threat model](06-security-and-threat-model.md).

Workflow transitions are not a runtime yet.
When they exist, they belong in this protobuf model so a run can be serialized and replayed.

## See also

- [Observability](05-observability.md)
- [Roadmap and phasing](09-roadmap-and-phasing.md)
