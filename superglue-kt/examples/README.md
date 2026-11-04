# Kotlin examples

The full number list is in [`../../examples/README.md`](../../examples/README.md).
Python scripts are in [`../../superglue-py/examples/`](../../superglue-py/examples/).
JavaScript scripts are in [`../../superglue-js/examples/`](../../superglue-js/examples/).

## Runnable JUnit tests

Live sections need `OPENAI_API_KEY`.
`OPENAI_MODEL` defaults to `gpt-6-luna` inside the test when unset.

From `superglue-kt/`:

```bash
./gradlew :superglue-android:testDebugUnitTest --tests 'com.superglue.kt.examples.*'
```

`superglue-android/build.gradle.kts` reads `OPENAI_API_KEY`, `OPENAI_MODEL`, and `OPENAI_BASE_URL` from `../../../.env` when the process environment does not set them.
From `superglue-android/`, that path is the directory that contains this repository when the checkout is `projects/superglue`.

| # | Topic | Test |
| --- | --- | --- |
| 01 | Simple completion | `example01_simple_completion` |
| 02 | System prompt | `example02_system_prompt` |
| 03 | Single tool | `example03_single_tool` |
| 04 | Multiple tools | `example04_multiple_tools` |
| 05 | Usage tracking | `example05_usage_tracking` |
| 06 | Error handling | `example06_error_handling` |
| 07 | Max tool rounds | `example07_max_tool_rounds` |
| 08 | Custom base URL | `example08_custom_base_url` |
| 09 | Concurrent completions | `example09_concurrent_completions` |
| 10 | Structured data tool | `example10_structured_data_tool` |
| 11 | Streaming | `example11_streaming` |
| 12 | Streaming accumulate | `example12_streaming_accumulate` |
| 13 | Concurrent streams | `example13_concurrent_streams` |
| 14 | Streaming vs buffered | `example14_streaming_vs_buffered` |
| 15 | Rate limiting | `example15_rate_limiting` |
| 16 | Retry configuration | `example16_retry_configuration` |
| 17 | Batch completions | `example17_batch_completions` |
| 18 | Hooks | `example18_hooks` |
| 19 | Guardrails | `example19_guardrails` |
| 20 | Agents | `example20_agents` |
| 21 | Executors | `example21_executors` (Python only) |
| 22 | Status events | `example22_status_events` |
| 23 | Reasoning effort | `example23_reasoning_effort` |
| 24 | Connection pool defaults | `example24_connection_pool_defaults` |
| 25 | Model fallback | `example25_model_fallback` |
| 26 | Stream response | `example26_stream_response` |
| 27 | Complete response | `example27_complete_response` |
| 28 | MCP tools | `example28_mcp_tools` |
| 29 | Multi-provider | `example29_multi_provider` |
| 30 | Streaming tool rounds | `example30_streaming_tools` |
| 31 | Inline file message | `example31_file_upload_inline` |
| 32 | Context optimization | `example32_context_optimization` |
| 34 | Live context comparison | `example34_context_optimization_live` |
| 35 | Code tool mode | `example35_code_tool_mode` |
| 36 | Realtime voice | `example36_realtime_voice` |
| 37 | TypeSafe System One | `example37_systemone` |
| demo | Short demo | `example_demo` |

`example37_systemone` needs `TYPESAFE_API_KEY`.
`example28_mcp_tools` runs only when `MCP_RUN=1`.

`example36_realtime_voice` uses OpenAI Realtime. `OPENAI_REALTIME_MODEL` overrides `gpt-realtime`.

There is no JUnit example for 33, 38, or 39.
Those rows use Rust bench fixtures, `audio::transcribe`, and `JsonlRunStore`.

## Client options used by the tests

- `ClientConfig.connectTimeoutSecs` default is 30. `poolMaxIdlePerHost` default is 50.
- `ClientConfig.reasoningEffort` sets the client reasoning effort.
- `ClientConfig.apiKeys`, `requestsPerSecondFor`, and `maxUploadBytes` set provider keys and limits.
- `AgentRunSpec.reasoningEffort` overrides one agent.
- `SuperglueClient.open(config, statusEmitter = …)` attaches a `StatusEmitter`.
- `client.complete(..., reasoningEffort = "high")` overrides one call.
- `client.uploadFile` and `messageWithFileBytes` attach files.
