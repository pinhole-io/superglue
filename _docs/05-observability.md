# Observability

## Warning

Traces and logs can leak prompts, tool output, and secrets.
The default scrub mode is `Redact`.
Use `Allow` only on a machine you control.

A build with the `prometheus` feature listens on `0.0.0.0:9090/metrics` when `init_metrics` runs.
The CLI calls `init_metrics` at startup.
Put a firewall in front of that port, or use `install_recorder` and serve `/metrics` on your own route.

## What is shipped

| Piece | Where |
| --- | --- |
| `tracing` spans | Core request, stream, tool, and retry paths |
| `init_tracing` | Pretty or JSON logs, `RUST_LOG`, scrub mode |
| `ScrubMode` | `Redact` (default), `Hash`, `Allow` |
| OpenInference attributes | `otlp` feature, `telemetry::openinference` |
| `StatusEmitter` | Opt-in `ProcessEvent` fan-out |
| `metrics` facade | Counters and histograms in `telemetry::metrics` |
| OTLP export | `otlp` feature and `TelemetryConfig::otlp_endpoint` |
| Prometheus | `prometheus` feature |

Hooks can change pipeline content.
Status events are observation only.
Attach an emitter with `ChatOptions.status_emitter` or the matching client option.

## OpenInference spans

Exported spans use the [OpenInference](https://github.com/Arize-ai/openinference/blob/main/spec/semantic_conventions.md) names.
`openinference.span.kind` is `AGENT`, `CHAIN`, `LLM`, `TOOL`, or `GUARDRAIL`.

| Span | Attributes |
| --- | --- |
| LLM | `llm.model_name`, `llm.provider`, `llm.system`, `llm.invocation_parameters`, flattened input and output messages, `llm.token_count.*` |
| Tool | `tool.name`, `tool.id`, and `session.id` |
| Agent | `agent.id` and `agent.name` from `AgentSpec::name`, plus `session.id` |
| Guardrail | Stage metadata, plus input and output text |

`session.id` is `ChatOptions.request_id`.
Set that id when one conversation must stay together.
An agent run creates a request id when the caller did not set one.

## Scrub rules

These rules apply to OpenInference attributes:

- Message text follows `ScrubMode`. `Redact` writes `[REDACTED]`. `Hash` writes a hash. `Allow` writes the text.
- A `tool` role message stays `[REDACTED]` in every mode.
- `function.arguments` on a tool call stays `[REDACTED]` in every mode.
- A `data:` URL stays `[data-uri]` in every mode.
- Each string attribute is cut at 16_384 characters.
- Roles, tool names, model ids, and token counts stay visible.

`set_input_text` and `set_output_text` follow `ScrubMode`.
Those helpers record the user text and the final reply. They do not record tool arguments.

Log fields named `api_key`, `authorization`, `content`, `arguments`, `result`, `prompt`, and `system_prompt` follow `ScrubMode`.
`Allow` prints those fields. `Redact` and `Hash` do not.

CLI flag: `--scrub-mode redact|hash|allow`.
An unknown value falls back to `redact` and logs a warning.

## Metrics

Names live in `src/telemetry/metrics.rs`.

| Name | Kind |
| --- | --- |
| `superglue.completions.total` | Counter. Label `model`. |
| `superglue.completions.errors` | Counter. Labels `model`, `error_kind`. |
| `superglue.tool_calls.total` | Counter. Label `tool_name`. |
| `superglue.tool_calls.errors` | Counter. Labels `tool_name`, `error_kind`. |
| `superglue.completion.duration_ms` | Histogram. Label `model`. |
| `superglue.stream.duration_ms` | Histogram. Label `model`. |
| `superglue.batch.size` | Histogram |
| `superglue.batch.duration_ms` | Histogram |
| `superglue.http.retries.total` | Counter |
| `superglue.system_one.total` | Counter. Label `model`. |
| `superglue.system_one.errors` | Counter. Labels `model`, `error_kind`. |
| `superglue.system_one.duration_ms` | Histogram. Label `model`. |

Without `prometheus`, `init_metrics` installs a no-op recorder so the macros do not panic.

## Errors

Rust errors use `thiserror` and keep the source chain.
Bindings map those types to host errors.

## See also

- [Security and threat model](06-security-and-threat-model.md)
- [Architecture](02-architecture.md)
- [Network privacy and telemetry shaping](07-network-privacy-and-telemetry-shaping.md)
