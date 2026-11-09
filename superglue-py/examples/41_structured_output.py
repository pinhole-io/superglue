"""Example 41: a completion that returns one JSON object checked against a schema.

    OPENAI_API_KEY=... uv run examples/41_structured_output.py
"""

import os

import superglue

api_key = os.environ.get("OPENAI_API_KEY", "")
if not api_key:
    print("Set OPENAI_API_KEY to run this example.")
    raise SystemExit(0)

base_url = os.environ.get("OPENAI_BASE_URL")
if base_url is None:
    base_url = (
        "https://api.openai.com" if api_key.startswith("sk-") else "http://localhost:11434"
    )

client = superglue.Client(
    api_key=api_key,
    model=os.environ.get("OPENAI_MODEL", "gpt-6-luna"),
    base_url=base_url,
)
person = client.complete_object(
    "Ada Lovelace, born in 1815, was an English mathematician.",
    {
        "name": "person",
        "description": "A person mentioned in the text.",
        "schema": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "born": {"type": "integer"},
                "occupation": {"type": "string"},
            },
            "required": ["name", "born", "occupation"],
            "additionalProperties": False,
        },
    },
)
print("name:", person.get("name"))
print("born:", person.get("born"))
print("occupation:", person.get("occupation"))
