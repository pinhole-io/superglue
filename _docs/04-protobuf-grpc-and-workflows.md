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

- The `AuditStore` trait, with `append`, `insert`, `get`, and `list`
- `MemoryRunStore`: bounded and in memory
- `JsonlRunStore`: one JSONL file of `WorkflowTransition` lines per request id
- `RunRecorder` and `ClientBuilder::audit(Arc<dyn AuditStore>)`
- `audit::resume_chat` and `audit::resume_response`
- `export_record_json` and `import_record_json` for metadata replay files

### Replayable run transitions

A unary run writes transitions while the tool loop runs:

1. `RUN_STARTED` holds the client messages and `options_json`. It resets the folded transcript.
2. `MESSAGE_APPENDED` holds one message. A tool round is written after all its tool results.
3. `RUN_FINISHED` or `RUN_FAILED` closes the run.

`get` folds the transitions into a `RunRecord`.
A run without `RUN_FINISHED` is a partial run. Pass it to `resume_chat` or `resume_response` to continue.

`JsonlRunStore` behavior:

- It calls `fsync` after each line.
- It skips a truncated final line.
- It drops an assistant tool-call round that has no complete set of tool results.
- It accepts only request ids that match `[A-Za-z0-9_-]+`.
- A disk error is logged. The completion continues.

This is not a general workflow engine. There are no branches, timers, or external triggers.

### Warning: a tool can run twice

A crash can occur after a tool runs and before its round is written.
Resume then runs that tool again.
Make side-effect tools idempotent, or key them on the request id and tool call id.

See [Security and threat model](06-security-and-threat-model.md) for the data on disk.

## See also

- [Observability](05-observability.md)
- [Roadmap and phasing](09-roadmap-and-phasing.md)
