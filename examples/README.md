# superglue Rust examples

Numbered examples match [`../superglue-py/examples/`](../superglue-py/examples/) and [`../superglue-js/examples/`](../superglue-js/examples/) when that language has the same number.

## Prerequisites

Export `OPENAI_API_KEY` before a live example.
`OPENAI_MODEL` overrides the helper default `gpt-6-luna`.
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
| 01 | Simple completion | yes | yes | yes | `example01_simple_completion` |
| 02 | System prompt | yes | yes | yes | `example02_system_prompt` |
| 03 | Single tool | yes | yes | yes | `example03_single_tool` |
| 04 | Multiple tools | yes | yes | yes | `example04_multiple_tools` |
| 05 | Usage tracking | yes | yes | yes | `example05_usage_tracking` |
| 06 | Error handling | yes | yes | yes | `example06_error_handling` |
| 07 | Max tool rounds | yes | yes | yes | `example07_max_tool_rounds` |
| 08 | Custom base URL | yes | yes | yes | `example08_custom_base_url` |
| 09 | Concurrent completions | yes | yes | yes | `example09_concurrent_completions` |
| 10 | Structured data tool | yes | yes | yes | `example10_structured_data_tool` |
| 11 | Streaming | yes | yes | yes | `example11_streaming` |
| 12 | Streaming accumulate | yes | yes | yes | `example12_streaming_accumulate` |
| 13 | Concurrent streams | yes | yes | yes | `example13_concurrent_streams` |
| 14 | Streaming vs buffered | yes | yes | yes | `example14_streaming_vs_buffered` |
| 15 | Rate limiting | yes | yes | yes | `example15_rate_limiting` |
| 16 | Retry configuration | yes | yes | yes | `example16_retry_configuration` |
| 17 | Batch completions | yes | yes | yes | `example17_batch_completions` |
| 18 | Hooks | yes | yes | yes | `example18_hooks` |
| 19 | Guardrails | yes | yes | yes | `example19_guardrails` |
| 20 | Agents | yes | yes | yes | `example20_agents` |
| 21 | Executors | stub | yes | stub | stub |
| 22 | Status events | yes | yes | yes | `example22_status_events` |
| 23 | Reasoning effort | yes | yes | yes | `example23_reasoning_effort` |
| 24 | Connection pool | yes | yes | yes | `example24_connection_pool_defaults` |
| 25 | Model fallback | yes | yes | yes | `example25_model_fallback` |
| 26 | Stream response | yes | yes | yes | `example26_stream_response` |
| 27 | Response threading | yes | yes | yes | `example27_complete_response` |
| 28 | MCP tools | yes | yes | yes | `example28_mcp_tools` |
| 29 | Multi-provider | yes | yes | yes | `example29_multi_provider` |
| 30 | Streaming tools | yes | yes | yes | `example30_streaming_tools` |
| 31 | File upload | yes | yes | yes | `example31_file_upload_inline` |
| 32 | Context optimization | yes | yes | yes | `example32_context_optimization` |
| 33 | Context benchmark, wiremock | yes | | | |
| 34 | Context benchmark, live API | yes | yes | yes | `example34_context_optimization_live` |
| 35 | Code tool mode | yes | yes | yes | `example35_code_tool_mode` |
| 36 | Realtime voice | yes | yes | yes | `example36_realtime_voice` |
| 37 | TypeSafe System One | yes | yes | yes | `example37_systemone` |
| 38 | File transcription | yes | yes | yes | `example38_file_transcription` |
| 39 | JSONL run store and resume | yes | | | |
| 40 | Judge config | yes | yes | yes | `example40_judge` |
| 41 | Structured output | yes | yes | yes | `example41_structured_output` |
| 42 | Advanced judge, builder | yes | yes | yes | `example42_judge_advanced` |
| 43 | Graph builder | yes | yes | yes | `example43_graph` |
| demo | Short demo | yes | yes | yes | `example_demo` |

Run Rust with `cargo run --example <name>`. Omit the `.rs` suffix.

| Example | Extra flag or variable |
| --- | --- |
| `28_mcp_tools` | `--features mcp`. Set `MCP_RUN=1` for a live server. |
| `33_context_optimization_benchmark` | No API key. Wiremock. |
| `34_context_optimization_live_benchmark` | `OPENAI_API_KEY` and `OPENAI_MODEL`. Python, JavaScript, and Kotlin compare two configs on one prompt. The Rust file also runs the fat-payload and multi-turn matrix. |
| `35_code_tool_mode` | `--features code` |
| `36_realtime_voice` | `--features realtime`. Needs `XAI_API_KEY`, or `SUPERGLUE_VOICE_PROVIDER=openai` with `OPENAI_API_KEY`. Skips when that key is unset. `OPENAI_REALTIME_MODEL` overrides `gpt-realtime`. Python, JavaScript, and Kotlin use OpenAI and run one scripted turn with a tool call. |
| `37_systemone` | `TYPESAFE_API_KEY` |
| `38_file_transcription` | Needs `XAI_API_KEY` by default. Skips when that key is unset. Uses `examples/sample.mp3` unless you pass another path. Set `SUPERGLUE_STT_PROVIDER` to `openai` or `groq` for those keys. Python, JavaScript, and Kotlin call `transcribe`. |
| `39_jsonl_resume` | `OPENAI_API_KEY`. Writes to a temporary directory. The example crashes a child run, then resumes it. |
| `40_judge` | `TYPESAFE_API_KEY`. Python, JavaScript, and Kotlin load the same judge config. |
| `41_structured_output` | `OPENAI_API_KEY`. Python, JavaScript, and Kotlin call `complete_object`. |
| `42_judge_advanced` | `TYPESAFE_API_KEY`. Python, JavaScript, and Kotlin send the same triage config. |
| `43_graph` | `OPENAI_API_KEY`. Drafts, then polishes. `TYPESAFE_API_KEY` adds a judge before the polish step. Python, JavaScript, and Kotlin send the same spec. |

## Rust `Client`

Examples use [`superglue::Client`](../src/client/mod.rs).
Shared helpers live in `examples/support/mod.rs`.

## Example 21

`SimpleExecutor`, `AgentExecutor`, and `AgentStructuredExecutor` exist only in Python.
The Rust file, the JavaScript file, and the Kotlin test `example21_executors` record that fact.
The Python script is [`../superglue-py/examples/21_executors.py`](../superglue-py/examples/21_executors.py).

## Rust-only rows

Python, JavaScript, and Kotlin have no API for these rows.

- **33** drives Wiremock through `benches/support`.
- **39** uses `JsonlRunStore`. The bindings do not expose an audit store.
