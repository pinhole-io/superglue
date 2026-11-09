"""Example 43: a graph that drafts, then polishes.

Set ``OPENAI_API_KEY``. The graph writes a short draft, then polishes it.
Set ``TYPESAFE_API_KEY`` to judge the draft before the polish step.

    OPENAI_API_KEY=... uv run examples/43_graph.py
"""

import os

import superglue

api_key = os.environ.get("OPENAI_API_KEY", "")
if not api_key:
    print("Set OPENAI_API_KEY to run this example.")
    raise SystemExit(1)

model = os.environ.get("OPENAI_MODEL", "gpt-6-luna")
base_url = os.environ.get("OPENAI_BASE_URL")
if base_url is None:
    base_url = (
        "https://api.openai.com" if api_key.startswith("sk-") else "http://localhost:11434"
    )
typesafe = os.environ.get("TYPESAFE_API_KEY", "")


def writer():
    return {
        "name": "writer",
        "persona": "a careful writer",
        "goals": [],
        "instructions": [],
        "constraints": [],
        "model": model,
        "max_tool_rounds": 16,
        "max_output_retries": 3,
        "system_prompt_override": None,
        "reasoning_effort": None,
    }


def draft_node():
    return {
        "id": "draft",
        "visits": 1,
        "step": {
            "kind": "agent",
            "spec": writer(),
            "prompt": "Write a short draft about {question}. Use at most 4 sentences.",
        },
    }


def polish_node(write=None):
    node = {
        "id": "polish",
        "visits": 1,
        "step": {
            "kind": "agent",
            "spec": writer(),
            "prompt": "Polish this draft. Keep it under 4 sentences.\n{draft}",
        },
    }
    if write is None:
        node["depends"] = ["draft"]
    else:
        node["write"] = write
    return node


if not typesafe:
    print("TYPESAFE_API_KEY is unset. The graph drafts, then polishes.")
    spec = {
        "name": "review",
        "require": ["question"],
        "output": "polish",
        "nodes": [draft_node(), polish_node()],
    }
    client = superglue.Client(api_key=api_key, model=model, base_url=base_url)
else:
    print("The graph drafts, judges, then polishes or keeps the draft.")
    spec = {
        "name": "review",
        "require": ["question"],
        "output": "answer",
        "nodes": [
            draft_node(),
            {
                "id": "grade",
                "visits": 1,
                "depends": ["draft"],
                "step": {
                    "kind": "judge",
                    "input": "draft",
                    "config": {
                        "id": "review.pass",
                        "model": "typesafe:jev-latest",
                        "timeout_ms": 400,
                        "on_error": "error",
                        "questions": {
                            "good": {
                                "type": "noul",
                                "instructions": "The draft answers the question in clear prose",
                            }
                        },
                        "outputs": [
                            {
                                "kind": "noul",
                                "name": "pass",
                                "from": "good",
                                "threshold": 0.5,
                                "required": False,
                            }
                        ],
                    },
                },
                "when": [{"path": "pass", "equals": True, "target": "polish"}],
                "otherwise": "keep",
            },
            polish_node("answer"),
            {
                "id": "keep",
                "write": "answer",
                "visits": 1,
                "step": {"kind": "map", "template": "{draft}"},
            },
        ],
    }
    client = superglue.Client(
        api_key=api_key,
        model=model,
        base_url=base_url,
        api_keys={"typesafe": typesafe},
    )

print("Question: Rust ownership")
print("Running the graph...")
answer = client.run_graph(spec, {"question": "Rust ownership"})
print("Answer:", answer)
