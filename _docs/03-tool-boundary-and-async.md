# Tool boundary and async

## Contract

The core calls a tool by name.
Arguments are JSON. Results are JSON.
The core does not parse host objects.

Each binding does three things:

1. Build a JSON Schema for the model.
2. Run the host function and return JSON.
3. Report failure as `ToolInvokeError`.

### Rust

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError>;
}
```

`ToolSpec` carries `name`, `description`, `parameters_schema`, and `static_tool`.
Set `static_tool` when dynamic or code mode must keep the tool without routing.
See [Context optimization](11-context-optimization.md).

### Python

The public helper is `Client.register_tool` in `superglue/__init__.py`.
It reads the function name, the docstring, and the annotations.
It then calls the PyO3 method, which only accepts a `dict` callback.

```python
def get_weather(location: str, unit: str = "celsius") -> dict:
    """Get the current weather for a city."""
    return {"location": location, "temperature": 22, "unit": unit}

client.register_tool(get_weather)
```

You can still pass `name`, `description`, `parameters`, and `static_tool`.
A function that takes one `args: dict` needs an explicit schema.

### JavaScript and Kotlin

Pass an explicit JSON Schema and a callback.
JavaScript guardrails must be synchronous. Tools may be async.
Prefer `superglue-js/facade` for option objects and guardrail tuples.

## Async model

The core is async on Tokio.
The Python binding blocks the calling thread with `block_on`.
Python tool functions are synchronous. `spawn_blocking` runs them where `Python::attach` is safe.

```text
Tokio loop (Python thread blocked in block_on)
  └── spawn_blocking
        └── Python::attach
              └── fn(args: dict) -> dict
```

Stream tokens use an `mpsc` channel plus the same blocking thread.
See [Architecture](02-architecture.md#python-token-delivery).

## Registry and dispatch

`ToolRegistry` stores `Arc<dyn Tool>` by name.
Registration is concurrent-safe.
One round dispatches every tool call in that round together.
`join_all` waits for the handlers.
The following `tool` messages follow `tool_calls` order, not finish order.
A streaming round may start a handler when its arguments are complete. The stored messages still follow `tool_calls` order.

## Error policy

`ToolInvokeError` has three variants:

| Variant | Meaning |
| --- | --- |
| `UnknownTool` | No tool has that name. |
| `DuplicateRegistration` | The name is already registered. |
| `HandlerFailed` | The handler returned an error. Optional `code`. |

`OnToolError` decides what the chat loop does after `HandlerFailed`:

| Policy | Behavior |
| --- | --- |
| `FailFast` | Abort the run. This is the default. |
| `Skip` | Append the error as the tool result and continue. |
| `Retry` | Retry with exponential backoff, then fail fast. |

`Retry` counts extra attempts. It does not count the first call.
The first delay is `initial_delay_ms`. Each later delay doubles.

## Tests

`tools::harness` runs a scripted plan with no network.
`tests/chat_tool_loop_wiremock.rs` serves fake tool-call responses.

## See also

- [Architecture](02-architecture.md)
- [Protobuf, gRPC, and workflows](04-protobuf-grpc-and-workflows.md)
