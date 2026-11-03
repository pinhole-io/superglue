# superglue Rust examples

Numbered examples match [`../superglue-py/examples/`](../superglue-py/examples/) and [`../superglue-js/examples/`](../superglue-js/examples/) when that language has the same number.

## Prerequisites

Export `OPENAI_API_KEY` before a live example.
`OPENAI_MODEL` overrides the helper default `gpt-5.4-nano`.
A bare model id uses OpenAI.

From this crate:

```bash
cargo run --example 01_simple_completion
```

`examples/support/mod.rs` picks the base URL:

- A key that starts with `sk-` uses `OPENAI_BASE_URL` or `https://api.openai.com`.
- Any other key uses `OPENAI_BASE_URL` or `http://localhost:11434`.

Set `OPENAI_BASE_URL` yourself when you want a specific host. See `08_custom_base_url`.

## Parity

| # | Topic | Rust | Python | JavaScript | Kotlin test |
| --- | --- | --- | --- | --- | --- |
| 01 | Simple completion | yes | yes | yes | |
| 02 | System prompt | yes | yes | yes | |
| 03 | Single tool | yes | yes | yes | |
| 04 | Multiple tools | yes | yes | yes | |
| 05 | Usage tracking | yes | yes | yes | |
| 06 | Error handling | yes | yes | yes | |
| 07 | Max tool rounds | yes | yes | yes | |
| 08 | Custom base URL | yes | yes | yes | |
| 09 | Concurrent completions | yes | yes | yes | |
| 10 | Structured data tool | yes | yes | yes | |
| 11 | Streaming | yes | yes | yes | |
| 12 | Streaming accumulate | yes | yes | yes | |
| 13 | Concurrent streams | yes | yes | yes | |
| 14 | Streaming vs buffered | yes | yes | yes | |
| 15 | Rate limiting | yes | yes | yes | |
| 16 | Retry configuration | yes | yes | yes | |
| 17 | Batch completions | yes | yes | yes | |
| 18 | Hooks | yes | yes | yes | |
| 19 | Guardrails | yes | yes | yes | |
| 20 | Agents | yes | yes | yes | |
| 21 | Executors | stub | yes | stub | |
| 22 | Status events | yes | yes | yes | `example22_status_events` |
| 23 | Reasoning effort | yes | yes | yes | `example23_reasoning_effort` |
| 24 | Connection pool | yes | yes | yes | `example24_connection_pool_defaults` |
| 25 | Model fallback | yes | yes | yes | `example25_model_fallback` |
| 26 | Stream response | yes | yes | yes | `example26_stream_response` |
| 27 | Response threading | yes | yes | yes | `example27_complete_response` |
| 28 | MCP tools | yes | yes | yes | |
| 29 | Multi-provider | yes | yes | yes | `example29_multi_provider` |
| 30 | Streaming tools | yes | yes | yes | `example30_streaming_tools` |
| 31 | File upload | yes | yes | yes | `example31_file_upload_inline` |
| 32 | Context optimization | yes | yes | yes | |
| 33 | Context benchmark, wiremock | yes | | | |
| 34 | Context benchmark, live API | yes | | | |
| 35 | Code tool mode | yes | yes | yes | |
| 36 | Realtime voice | yes | | | |
| 37 | TypeSafe System One | yes | | | |
| demo | Short demo | yes | yes | yes | |

Run Rust with `cargo run --example <name>`. Omit the `.rs` suffix.

| Example | Extra flag or variable |
| --- | --- |
| `28_mcp_tools` | `--features mcp`. Set `MCP_RUN=1` for a live server. |
| `33_context_optimization_benchmark` | No API key. Wiremock. |
| `34_context_optimization_live_benchmark` | `OPENAI_API_KEY` and `OPENAI_MODEL` |
| `35_code_tool_mode` | `--features code` |
| `36_realtime_voice` | `--features realtime` and `XAI_API_KEY` |
| `37_systemone` | `TYPESAFE_API_KEY` |

## Rust `Client`

Examples use [`superglue::Client`](../src/client/mod.rs).
Shared helpers live in `examples/support/mod.rs`.

## Example 21

`SimpleExecutor`, `AgentExecutor`, and `AgentStructuredExecutor` exist only in Python.
The Rust and JavaScript files print that fact.
The Python script is [`../superglue-py/examples/21_executors.py`](../superglue-py/examples/21_executors.py).
