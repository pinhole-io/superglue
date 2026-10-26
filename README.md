# SuperGlue

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust edition 2024](https://img.shields.io/badge/rust-edition%202024-orange.svg)](Cargo.toml)
[![Python 3.12+](https://img.shields.io/badge/python-3.12%2B-blue.svg)](superglue-py)
[![Node.js 18+](https://img.shields.io/badge/node.js-18%2B-green.svg)](superglue-js)
[![Kotlin JNI](https://img.shields.io/badge/kotlin-Android%20JNI-purple.svg)](superglue-kt)

**LLM orchestration in one Rust core.** Python, JavaScript, and Kotlin call the same loop.

SuperGlue talks to model providers, runs tool calling, streams tokens, and records usage. Retry rules, SSE parsing, and tool dispatch live in Rust. Bindings register tools and callbacks. Tool arguments and results cross the boundary as JSON.

[Quick start](#quick-start) · [Install](#install) · [Providers](docs/multi-provider.md) · [Gateway](_docs/12-llm-gateway.md) · [Examples](examples/README.md) · [Design notes](_docs/README.md)

> Release `1.0.0`. The workflow engine is on the [roadmap](_docs/09-roadmap-and-phasing.md).

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Tool calling](#tool-calling)
- [Features](#features)
- [How it fits together](#how-it-fits-together)
- [Model providers](#model-providers)
- [LLM gateway](#llm-gateway)
- [Documentation](#documentation)
- [Examples](#examples)
- [Troubleshooting](#troubleshooting)
- [Development](#development)
- [License](#license)

## Install

Pick the language you call from. Every package shares `[workspace.package].version` in [Cargo.toml](Cargo.toml). See [PACKAGES.md](PACKAGES.md).

| Language | Where it runs | Package |
| --- | --- | --- |
| Rust | Library and CLI | This git repository. There is no crates.io package. |
| Python | CPython 3.12 or newer | `pip install superglue-py` |
| JavaScript | Node.js 18 or newer. Linux x86_64, Linux aarch64, macOS x86_64, macOS Apple Silicon. | `npm install superglue-js` |
| Kotlin | Android JNI (`arm64-v8a`, `x86_64`) | `com.superglue.kt:superglue-android` |

Optional Cargo features stay off until you enable them.

| Feature | What it adds |
| --- | --- |
| `mcp` | MCP tool clients |
| `grpc` | gRPC server and client |
| `gateway` | OpenAI-compatible HTTP gateway (virtual keys, budgets, usage) |
| `capture` | Gateway traffic capture. Requires `gateway`. Can upload to S3. |
| `otlp` | OpenTelemetry span export |
| `prometheus` | Prometheus metrics endpoint |
| `code` | Programmatic JavaScript tool mode |
| `realtime` | Realtime voice sessions. The first adapter is xAI Speech to Speech. |

### Rust

Add the crate from git:

```toml
[dependencies]
superglue = { git = "https://github.com/pinhole-io/superglue" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Pin a release with `tag = "v1.0.0"`. Enable a feature in the same dependency:

```toml
superglue = { git = "https://github.com/pinhole-io/superglue", features = ["gateway"] }
```

### Python

Install the package from PyPI. The import name is `superglue`.

```bash
pip install superglue-py
```

Wheels cover CPython 3.12, 3.13, and 3.14, including free-threaded 3.14, on Linux x86_64, Linux aarch64, macOS x86_64, and macOS Apple Silicon. The source distribution is on PyPI for other platforms. A Rust toolchain is required to build that source distribution.

Each GitHub release also stores the wheels. Download the wheel for your platform, then install that file:

```bash
pip install ./superglue_py-*.whl
```

To build from this repository:

```bash
cd superglue-py
uv venv --python 3.14t
./dev.sh
```

`requires-python` is `>=3.12`. `./dev.sh` looks for `.venv/bin/python` from that 3.14t environment.

### JavaScript

Install the package from npm:

```bash
npm install superglue-js
```

Prebuilt binaries cover Linux x86_64, Linux aarch64, macOS x86_64, and macOS Apple Silicon. On another platform, clone this repository and build the addon:

```bash
cd superglue-js
npm install
npm run build
```

The GitHub release also stores each `.node` file. Point `NAPI_RS_NATIVE_LIBRARY_PATH` at the file for your platform when you load a local checkout of the package.

Tools need an explicit JSON Schema. Prefer `superglue-js/facade` for client options, tools, and guardrails. Details: [superglue-js/README.md](superglue-js/README.md).

### Kotlin

Add the Android library from Maven Central:

```kotlin
dependencies {
    implementation("com.superglue.kt:superglue-android:1.0.0")
}
```

The AAR contains `arm64-v8a` and `x86_64`. Each GitHub release also stores `superglue-android-1.0.0.aar`.

A file dependency does not pull AndroidX or kotlinx. Add those libraries yourself, or include the `:superglue-android` module from this repository.

```kotlin
dependencies {
    implementation(files("libs/superglue-android-1.0.0.aar"))
}
```

Build steps: [superglue-kt/README.md](superglue-kt/README.md).

## Quick start

Set a provider key before you call a live model. The examples use `OPENAI_API_KEY`.

Keep keys in the environment. The core stores them in `secrecy` types and redacts them in traces.

### From this repository

```bash
cargo run --example 01_simple_completion
```

### Rust library

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = superglue::Client::builder()
        .api_key(std::env::var("OPENAI_API_KEY")?)
        .model("openai:gpt-4o-mini")
        .build()?;

    let outcome = client
        .complete(
            "What is the capital of France?",
            superglue::CallOptions::default(),
        )
        .await?;

    println!("{}", outcome.content.unwrap_or_default());
    Ok(())
}
```

Pass models as `provider:model`. Examples: `openai:gpt-4o-mini`, `anthropic:claude-sonnet-4-20250514`.

### Python

```python
import os
import superglue

client = superglue.Client(
    api_key=os.environ["OPENAI_API_KEY"],
    model="openai:gpt-4o-mini",
)
result = client.complete("What is the capital of France? Reply in one sentence.")
print(result.content)
```

### JavaScript

Run this after `npm run build` in `superglue-js/`.

```js
const { createClient } = require("superglue-js/facade");

async function main() {
  const client = createClient({
    apiKey: process.env.OPENAI_API_KEY,
    model: "openai:gpt-4o-mini",
  });
  const outcome = await client.complete("What is the capital of France?");
  console.log(outcome.content);
}

main();
```

## Tool calling

Register a tool on the client. `complete` runs the tool loop in Rust and returns the final text.

Python builds the JSON Schema from the function signature and the docstring:

```python
def get_weather(location: str, unit: str = "celsius") -> dict:
    """Get the current weather for a city."""
    return {"location": location, "temperature": 22, "unit": unit}

client.register_tool(get_weather)
result = client.complete("What is the weather in Tokyo?")
print(result.content)
```

JavaScript and Kotlin need an explicit JSON Schema on each tool. Rust examples use `ToolSpec` plus a callback. Start with [examples/03_single_tool.rs](examples/03_single_tool.rs), [superglue-py/examples/03_single_tool.py](superglue-py/examples/03_single_tool.py), and [superglue-js/examples/03_single_tool.mjs](superglue-js/examples/03_single_tool.mjs).

Each tool can use `FailFast`, `Skip`, or `Retry`. One round can dispatch several tools together.

## Features

| Feature | What you get |
| --- | --- |
| Completions | Buffered `complete` on one client |
| Streaming | SSE `stream`. With tools registered, the stream runs the same multi-round loop. |
| Tool loop | JSON tools, parallel dispatch in a round, per-tool error policy |
| Providers | `provider:model` routing. See [Model providers](#model-providers). |
| Embeddings | `Client::embed` and gateway `POST /v1/embeddings` for OpenAI-compatible providers |
| Reliability | Retries, per-key rate limits, model fallback, cooperative cancel |
| Agents | Personas, hooks, guardrails, status events, usage, and cost |
| Context control | Dynamic tool routing, round condensing, and optional code-mode tools (`code` feature) |
| Files | Inline bytes or a provider file upload. Default inline cap is 20 MB. |
| Voice | Duplex sessions on xAI when you enable `realtime` |
| Observability | `tracing` with redaction. Optional OTLP and Prometheus. |
| Servers | gRPC sidecar (`grpc`) and an OpenAI-compatible HTTP gateway (`gateway`) |
| Bindings | Python (PyO3), Node.js (napi-rs), Kotlin (JNI) |

## How it fits together

```text
Python / JavaScript / Kotlin / CLI
            │  JSON tools, native callbacks
            ▼
     SuperGlue (Rust, Tokio)
            │  HTTP, SSE, optional WebSocket
            ▼
     Model providers
```

- The Rust core owns HTTP, retries, streaming, the tool loop, and telemetry.
- A binding supplies the host callback and the JSON payload.
- `proto/superglue.proto` is the shared schema for gRPC and audit records.
- The gateway is an optional HTTP front door. It adds virtual keys, budgets, and usage, then calls the same providers.

Read the map in [Architecture](_docs/02-architecture.md).

## Model providers

Use `provider:model` for chat. The prefix selects the key and the base URL. The suffix is the model id sent to that provider. A bare model id uses OpenAI.

| Provider | Model prefix | API key variable |
| --- | --- | --- |
| OpenAI | `openai` | `OPENAI_API_KEY` |
| Anthropic | `anthropic` | `ANTHROPIC_API_KEY` |
| xAI | `xai` | `XAI_API_KEY` |
| Groq | `groq` | `GROQ_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
| RunInfra | `runinfra` | `RUNINFRA_GATEWAY_KEY` |
| Vercel AI Gateway | `vercel` | `VERCEL_GATEWAY_KEY` |

OpenRouter also accepts `OPENROUTER_GATEWAY_KEY` when `OPENROUTER_API_KEY` is unset.

The environment loader reads two base URL overrides: `OPENAI_BASE_URL` and `TYPESAFE_BASE_URL`. Set `OPENAI_BASE_URL` for an OpenAI-compatible host such as Ollama. Set any other base URL on the client with `ClientBuilder::base_url_for` in Rust, or the matching option in a binding.

TypeSafe (`TYPESAFE_API_KEY`) is the System One evaluation API. It is separate from the chat providers above.

Embeddings use `provider:embedding:model` when the model must appear in an embedding catalog. Example: `openai:embedding:text-embedding-3-small`.

Voice example: `xai:grok-voice-think-fast-2.0` with the `realtime` feature and `XAI_API_KEY`.

File uploads, per-key rate limits, and binding overrides: [docs/multi-provider.md](docs/multi-provider.md).

## LLM gateway

The `gateway` feature serves an OpenAI-compatible HTTP proxy. It stores virtual keys, budgets, and usage in SQLite.

**Keep `GATEWAY_MASTER_KEY` on the operator host.** It authorizes admin commands.

```bash
cargo build --features gateway --release

export GATEWAY_MASTER_KEY="replace-me"
export OPENAI_API_KEY="sk-..."

./target/release/superglue gateway serve --addr 127.0.0.1:8080 --db ./superglue-gateway.db
```

The gateway binary embeds the admin UI. Build [gateway-admin](gateway-admin/README.md) before `cargo build` when you want `/admin/`.

### CLI cheat sheet

Run these with `./target/release/superglue` after the build above.

Global flags: `--log-level`, `--log-format pretty|json`, `--scrub-mode redact|hash|allow`.

| Task | Command |
| --- | --- |
| Start the server | `superglue gateway serve --addr 127.0.0.1:8080` |
| Create a budget | `superglue gateway budget create --max-budget 10 --duration-sec 2592000` |
| Create a user | `superglue gateway user create --user-id alice --alias Alice` |
| Create a virtual key | `superglue gateway key create --user-id alice --model 'openai:*'` |
| List keys | `superglue gateway key list` |
| List usage | `superglue gateway usage list --user-id alice --limit 50` |
| List models for a key | `superglue gateway model list --key sgw-...` |
| Talk to a remote gateway | `superglue gateway --url https://gateway.example.com user list` |

Key creation prints the secret once. Store it before you close the terminal.

Full command list, profiles, and threat notes: [_docs/12-llm-gateway.md](_docs/12-llm-gateway.md).

## Documentation

Design notes live in this repo. Start at [_docs/README.md](_docs/README.md).

| Goal | Start here |
| --- | --- |
| See why the core is Rust | [Vision and principles](_docs/01-vision-and-principles.md) |
| See what runs where | [Architecture](_docs/02-architecture.md) |
| Pass tools across a binding | [Tool boundary and async](_docs/03-tool-boundary-and-async.md) |
| Call another provider or attach a file | [Multi-provider guide](docs/multi-provider.md) |
| Shrink a long tool transcript | [Context optimization](_docs/11-context-optimization.md) |
| Run the HTTP gateway | [LLM gateway](_docs/12-llm-gateway.md) |
| Open the admin UI | [Gateway admin](gateway-admin/README.md) |
| Export traces or metrics | [Observability](_docs/05-observability.md) |
| Review secrets, auth, and failure | [Security and threat model](_docs/06-security-and-threat-model.md) |
| Use gRPC or the protobuf schema | [Protobuf, gRPC, and workflows](_docs/04-protobuf-grpc-and-workflows.md) |
| See what is shipped | [Design index](_docs/README.md) |
| See what is planned | [Roadmap](_docs/09-roadmap-and-phasing.md) |
| Run a numbered example | [Examples](examples/README.md) |
| Bump the shared version | [PACKAGES.md](PACKAGES.md) |
| Publish PyPI, npm, and Maven | [PACKAGES.md](PACKAGES.md) |

## Examples

Numbered examples cover the same topics in Rust, Python, and JavaScript.

```bash
cargo run --example 01_simple_completion
cargo run --example 11_streaming
cargo run --example 29_multi_provider
cargo run --example 28_mcp_tools --features mcp
cargo run --example 35_code_tool_mode --features code
cargo run --example 36_realtime_voice --features realtime
```

| Goal | Example |
| --- | --- |
| First completion | `01_simple_completion` |
| System prompt | `02_system_prompt` |
| One tool, then several | `03_single_tool`, `04_multiple_tools` |
| Usage, errors, retry, rate limits | `05_usage_tracking`, `06_error_handling`, `15_rate_limiting`, `16_retry_configuration` |
| Streaming | `11_streaming`, `30_streaming_tools` |
| Batch calls | `17_batch_completions` |
| Hooks, guardrails, agents | `18_hooks`, `19_guardrails`, `20_agents` |
| Fallback and several providers | `25_model_fallback`, `29_multi_provider` |
| Files | `31_file_upload` |
| Smaller tool context | `32_context_optimization`, `35_code_tool_mode` |
| MCP tools | `28_mcp_tools` |
| Voice | `36_realtime_voice` |

The full parity table is in [examples/README.md](examples/README.md).

## Troubleshooting

| What you see | What to do |
| --- | --- |
| Missing `OPENAI_API_KEY` | Export the key for the provider prefix you used. See [Model providers](#model-providers). |
| The call hits OpenAI with the wrong model id | Use `provider:model`, for example `anthropic:claude-sonnet-4-20250514`. A slash form logs a deprecation warning. An unknown prefix falls back to OpenAI. |
| `unrecognized subcommand 'gateway'` | Rebuild with `--features gateway`. The default binary omits that command. |
| MCP example fails to compile | Pass `--features mcp`. |
| Code-mode example fails to compile | Pass `--features code`. |
| Python cannot import `superglue` | Run `./dev.sh` inside `superglue-py` after `uv venv --python 3.14t`. |
| Node `Cannot find module` | Run `npm run build` in `superglue-js`. The facade import also needs `npm run build:ts`. |
| A secret appears in a log | Keep the default scrub mode `redact`. Use `--scrub-mode allow` only on a local machine. |
| Gateway admin page is empty | Build [gateway-admin](gateway-admin/README.md), then rebuild the `gateway` feature. The binary embeds `dist/`. |

## Development

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

MCP tests need the feature flag:

```bash
cargo test --features mcp
```

After you change `[workspace.package].version`:

```bash
./scripts/sync-superglue-versions.sh
./scripts/check-superglue-version-parity.sh
```

Publish that version with a GitHub Release tagged `v` plus the workspace version. Steps and secrets: [PACKAGES.md](PACKAGES.md).

In [thestack](https://github.com/Bioto/thestack), this repository is the `projects/superglue` submodule. Layout notes: [MONOREPO.md](MONOREPO.md).

## License

The license is [MIT](LICENSE).

Keep the copyright notice and the permission notice in every copy.
