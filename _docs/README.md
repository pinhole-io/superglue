# Superglue design documentation

These notes describe intent, tradeoffs, and the current implementation.
The Rust crate is the parent directory.
Bindings live in `superglue-py/`, `superglue-js/`, and `superglue-kt/`.

The public guide starts at [`../README.md`](../README.md).
Provider routing is in [`../docs/multi-provider.md`](../docs/multi-provider.md).

## Reading order

1. [Vision and principles](01-vision-and-principles.md)
2. [Architecture](02-architecture.md)
3. [Tool boundary and async](03-tool-boundary-and-async.md)
4. [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md)
5. [Observability](05-observability.md)
6. [Security and threat model](06-security-and-threat-model.md)
7. [Network privacy and telemetry shaping](07-network-privacy-and-telemetry-shaping.md)
8. [Operations: throttled downloads](08-operations-throttled-downloads.md)
9. [Roadmap and phasing](09-roadmap-and-phasing.md)
10. [Ecosystem reference](10-ecosystem-reference.md)
11. [Context optimization](11-context-optimization.md)
12. [LLM gateway](12-llm-gateway.md)

## Current implementation status

| Component | Status |
| --- | --- |
| HTTP client, retries, rate limits, SSE parser | Shipped |
| Tool registry, parallel dispatch, per-tool error policy | Shipped |
| Chat completions and the Responses API | Shipped |
| Streaming, including tool rounds | Shipped |
| `Client` and `ClientConversation` | Shipped |
| Embeddings | Shipped |
| File attachments and image parts | Shipped |
| File transcription (`audio`) | Shipped for OpenAI and Groq |
| Batch, hooks, guardrails, agents | Shipped |
| Model fallback and `provider:model` routing | Shipped |
| Context optimization and code mode (`code` feature) | Shipped |
| MCP tools (`mcp` feature) | Shipped |
| Realtime voice (`realtime` feature, xAI and OpenAI adapters) | Shipped |
| Voice sessions on the Python, Node.js, and Kotlin bindings | Shipped |
| Replayable run transitions and `JsonlRunStore` | Shipped |
| Gateway budget holds for in-flight requests | Shipped |
| Protobuf schema and gRPC server (`grpc` feature) | Shipped |
| Audit records and resume helpers | Shipped |
| Tracing scrub, OTLP (`otlp`), Prometheus (`prometheus`) | Shipped |
| LLM gateway (`gateway` feature) | Shipped |
| Gateway traffic capture (`capture` feature) | Shipped |
| Python, Node.js, and Kotlin bindings | Shipped |
| General workflow engine (branches, timers, triggers) | Out of scope. Replayable transitions cover resume. |
| Traffic shaping from [note 07](07-network-privacy-and-telemetry-shaping.md) | Not a product requirement |

## Related code

| Area | Location |
| --- | --- |
| Rust core | [`../src`](../src) |
| Protobuf schema | [`../proto/superglue.proto`](../proto/superglue.proto) |
| Python binding | [`../superglue-py`](../superglue-py) |
| Node.js binding | [`../superglue-js`](../superglue-js) |
| Kotlin binding | [`../superglue-kt`](../superglue-kt) |
| Examples | [`../examples/README.md`](../examples/README.md) |
| Gateway admin UI | [`../gateway-admin/README.md`](../gateway-admin/README.md) |
| Gateway host deploy | [`../deploy/README.md`](../deploy/README.md) |
