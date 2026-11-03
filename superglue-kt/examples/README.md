# Kotlin examples

The full number list is in [`../../examples/README.md`](../../examples/README.md).
Python scripts are in [`../../superglue-py/examples/`](../../superglue-py/examples/).
JavaScript scripts are in [`../../superglue-js/examples/`](../../superglue-js/examples/).

## Runnable JUnit tests

Live sections need `OPENAI_API_KEY`.
`OPENAI_MODEL` defaults to `gpt-5.4-nano-2026-03-17-mini` inside the test when unset.

From `superglue-kt/`:

```bash
./gradlew :superglue-android:testDebugUnitTest --tests 'com.superglue.kt.examples.FeatureExamplesTest'
```

`superglue-android/build.gradle.kts` reads `OPENAI_API_KEY`, `OPENAI_MODEL`, and `OPENAI_BASE_URL` from `../../../.env` when the process environment does not set them.
From `superglue-android/`, that path is the directory that contains this repository when the checkout is `projects/superglue`.

| # | Topic | Test |
| --- | --- | --- |
| 22 | Status events | `example22_status_events` |
| 23 | Reasoning effort | `example23_reasoning_effort` |
| 24 | Connection pool defaults | `example24_connection_pool_defaults` |
| 25 | Model fallback | `example25_model_fallback` |
| 26 | Stream response | `example26_stream_response` |
| 27 | Complete response | `example27_complete_response` |
| 29 | Multi-provider | `example29_multi_provider` |
| 30 | Streaming tool rounds | `example30_streaming_tools` |
| 31 | Inline file message | `example31_file_upload_inline` |

There is no JUnit example for 01–21, 28, or 32–37.

## Client options used by the tests

- `ClientConfig.connectTimeoutSecs` default is 30. `poolMaxIdlePerHost` default is 50.
- `ClientConfig.reasoningEffort` sets the client reasoning effort.
- `ClientConfig.apiKeys`, `requestsPerSecondFor`, and `maxUploadBytes` set provider keys and limits.
- `AgentRunSpec.reasoningEffort` overrides one agent.
- `SuperglueClient.open(config, statusEmitter = …)` attaches a `StatusEmitter`.
- `client.complete(..., reasoningEffort = "high")` overrides one call.
- `client.uploadFile` and `messageWithFileBytes` attach files.
