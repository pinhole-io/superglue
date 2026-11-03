# Ecosystem reference

A crate is listed with the Cargo feature that enables it.
No feature means the default build.

## HTTP and streaming

| Crate | Feature | Role |
| --- | --- | --- |
| `reqwest` | default | Async HTTP. JSON, rustls, multipart, query, byte streams. |
| `bytes` | default | Streaming buffers |
| `futures-util` | default | Stream and `join_all` helpers |
| `tower`, `tower-http`, `axum` | `gateway` | HTTP gateway and middleware |
| `tokio-tungstenite`, `rustls`, `url` | `realtime` | Voice WebSocket |

SSE parsing is in `src/http/sse.rs`. It is not a separate crate.

## Serialization

| Crate | Feature | Role |
| --- | --- | --- |
| `serde`, `serde_json` | default | Provider JSON and tool payloads |
| `prost` | default | Protobuf runtime |
| `prost-build`, `protoc-bin-vendored` | build | Generate Rust from `proto/superglue.proto` |
| `tonic`, `tonic-prost`, `tokio-stream` | `grpc` | gRPC server. `tokio-stream` is also a `gateway` dependency. |

## Async and limits

| Crate | Feature | Role |
| --- | --- | --- |
| `tokio` | default | Runtime, timers, `mpsc`, `spawn_blocking` |
| `async-trait` | default | `Tool` trait objects |
| `tokio-util` | default | `CancellationToken` |
| `governor` | default | Token bucket for QPS and downloads |

## Gateway storage and capture

| Crate | Feature | Role |
| --- | --- | --- |
| `rusqlite` | `gateway` | SQLite database. Bundled SQLite. |
| `subtle` | `gateway` | Constant-time key compare |
| `chacha20poly1305` | `gateway` | Encrypt stored proxy API keys |
| `rust-embed` | `gateway` | Embed the admin UI |
| `chrono` | `gateway` | Timestamps. Also used by `mcp`. |
| `aws-config`, `aws-sdk-s3`, `aws-smithy-async`, `flate2` | `capture` | Gzip spool and S3 upload |

## Other optional crates

| Crate | Feature | Role |
| --- | --- | --- |
| `rmcp`, `http` | `mcp` | MCP client transports |
| `boa_engine` | `code` | JavaScript tool mode |
| `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, `tracing-opentelemetry` | `otlp` | OTLP span export |
| `metrics-exporter-prometheus` | `prometheus` | Prometheus scrape recorder |

## Always present

| Crate | Role |
| --- | --- |
| `tracing`, `tracing-subscriber` | Spans, env filter, pretty and JSON logs |
| `metrics` | Counters and histograms. No-op recorder unless `prometheus` is on. |
| `thiserror` | Typed errors |
| `secrecy`, `zeroize` | API key storage |
| `clap` | CLI |
| `uuid` | Request ids |
| `regex`, `base64`, `sha2` | Parsing, file bytes, image hashes |

`rustls` is the TLS stack through `reqwest`. The release profile enables LTO and strips symbols.

## Language bindings

| Crate | Package | Role |
| --- | --- | --- |
| `pyo3` 0.29 | `superglue-py` | Python extension. `maturin` builds the wheel. |
| `napi` 3 | `superglue-js` | Node.js addon |
| `jni` 0.21 | `superglue-kt/native` | Android JNI |

Python wheels target CPython 3.12, 3.13, and 3.14, including free-threaded 3.14.
`superglue-py/dev.sh` builds against `.venv/bin/python`.

`pyo3-async-runtimes` is not a dependency. The Python binding uses `block_on` and `spawn_blocking`.

## Tests

| Crate | Role |
| --- | --- |
| `wiremock` | Fake provider HTTP |
| `assert_cmd`, `predicates` | CLI tests |
| `tempfile` | Temporary files |
| `criterion` | Context-optimization benches |
| `http-body-util` | Gateway body tests |

## Prior art

- `async-openai` for streaming shapes and OpenAI types
- `ruff` and `uv` for PyO3 packaging layout

## See also

- [Architecture](02-architecture.md)
- [Roadmap and phasing](09-roadmap-and-phasing.md)
