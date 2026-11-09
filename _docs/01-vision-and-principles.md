# Vision and principles

## Context

`superglue` is the Rust orchestration core.
Python, JavaScript, and Kotlin call the same loop.
The license is MIT.

## Primary goal

One implementation owns provider I/O, retries, streaming, and the tool loop.
A fix lands in Rust and reaches every binding.

## Why Rust for the core

- Tokio runs network work, concurrent tools, and batches without a Python GIL on the hot path.
- The same core ships as a library, a PyO3 module, a napi addon, a JNI library, or a gRPC sidecar.
- `reqwest`, `serde`, `prost`, and `tracing` match this problem.

## Design principles

1. Orchestration lives in Rust. The core calls providers, parses SSE, retries, and dispatches tools.
2. Bindings stay thin. They register tools and map host types. They do not reimplement the loop.
3. Tool arguments and tool results cross the binding as JSON.
4. Protobuf is the schema for gRPC and audit records. See [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md).
5. The core is async on Tokio.
6. The Python binding blocks the caller with `block_on`. Tool callbacks and stream tokens use `spawn_blocking`.
7. Traces and metrics start in Rust. Bindings adapt export to the host.
8. Wheels support CPython 3.12 and newer, including free-threaded 3.14.

## Non-goals

- Rust does not introspect Python functions or Pydantic models. The Python package builds JSON Schema. The core consumes JSON.
- These notes are not a field-by-field API dump. Concrete APIs live in the code and in the language READMEs.

## Open decisions

- A language can use in-process FFI or the gRPC sidecar. See [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md).
- Graph composition is in the crate. A durable workflow engine stays out of scope. See [Roadmap and phasing](09-roadmap-and-phasing.md).
