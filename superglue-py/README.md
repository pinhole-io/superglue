# superglue-py

Python bindings for the [superglue](https://github.com/pinhole-io/superglue) Rust core.

The import name is `superglue`. The package name is `superglue-py`.

## Install

```bash
pip install superglue-py
```

Wheels cover CPython 3.12, 3.13, and 3.14, including free-threaded 3.14.
Platforms: Linux x86_64, Linux aarch64, macOS x86_64, and macOS Apple Silicon.

On another platform, install the source distribution from PyPI.
You need a Rust toolchain.
You can also download a wheel from the GitHub release and pass that file to `pip install`.

## Build from this repository

```bash
uv venv --python 3.14t
./dev.sh
```

`requires-python` is `>=3.12`.
`./dev.sh` uses `.venv/bin/python`.

## Call a model

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

## Tools

`register_tool` reads the function name, the docstring, and the type annotations.
The Rust core still receives JSON.

```python
def get_weather(location: str, unit: str = "celsius") -> dict:
    """Get the current weather for a city."""
    return {"location": location, "temperature": 22, "unit": unit}

client.register_tool(get_weather)
result = client.complete("What is the weather in Tokyo?")
print(result.content)
```

Pass `name`, `description`, `parameters`, or `static_tool` when you need to override the inferred schema.
A function that takes one `args: dict` needs an explicit schema.

## What else the client does

- `stream` delivers tokens. With tools registered, it runs the tool loop.
- `batch`, hooks, guardrails, and agents match the Rust client.
- `api_keys` and `requests_per_second_for` select providers. See [`../docs/multi-provider.md`](../docs/multi-provider.md).
- `upload_file` and `message_with_file_bytes` attach files.
- `voice_connect` opens a realtime voice session on OpenAI or xAI. `on_event` runs on a native thread. See [`../docs/multi-provider.md`](../docs/multi-provider.md#realtime-voice).
- Executors (`SimpleExecutor`, `AgentExecutor`, `AgentStructuredExecutor`) exist only in this package.

The native crate is built with the `code`, `mcp`, and `realtime` features.

## Examples

Numbered scripts live in [`examples/`](examples/).
The parity table is in [`../examples/README.md`](../examples/README.md).

```bash
export OPENAI_API_KEY=sk-...
uv run examples/01_simple_completion.py
```
