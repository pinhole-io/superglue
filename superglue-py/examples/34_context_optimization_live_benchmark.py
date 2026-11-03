"""Example 34: Live context-optimization comparison.

Runs one prompt on a standard tool client and on a dynamic, condensed client.
Prints rounds and token usage for each.

The Rust example also runs the fat-payload and multi-turn matrix from the
bench harness. Set `SUPERGLUE_BENCH_ONLY` there to select a section.

    OPENAI_API_KEY=sk-... uv run examples/34_context_optimization_live_benchmark.py
"""

import os

import superglue

api_key = os.environ.get("OPENAI_API_KEY", "")
if not api_key:
    print("Set OPENAI_API_KEY to run this example.")
    raise SystemExit(1)

model = os.environ.get("OPENAI_MODEL", "gpt-6-luna")
prompt = "What's the weather in Paris?"


def get_weather(city: str) -> dict:
    """Get weather for a city."""
    return {"city": city, "temp_f": 72, "condition": "sunny"}


def calculate(expression: str) -> dict:
    """Evaluate a math expression."""
    return {"expression": expression, "result": 42}


def get_time() -> dict:
    """Return current UTC time (pinned static tool)."""
    return {"time": "12:00Z"}


def run(label: str, *, tool_mode: str = "standard", condense_tool_messages: bool = False) -> None:
    client = superglue.Client(
        api_key=api_key,
        model=model,
        system_prompt="You are a helpful assistant. Use tools when needed.",
        tool_mode=tool_mode,
        condense_tool_messages=condense_tool_messages,
    )
    client.register_tool(get_weather)
    client.register_tool(calculate)
    client.register_tool(get_time, static_tool=True)
    result = client.complete(prompt, reasoning_effort="none")
    print(f"{label} rounds:", result.rounds)
    print(f"{label} usage:", result.usage)
    print(f"{label} content:", result.content)


run("standard")
run("optimized", tool_mode="dynamic", condense_tool_messages=True)
