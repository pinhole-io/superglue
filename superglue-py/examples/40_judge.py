"""Example 40: a judge config that maps a Noul answer to a bool.

    TYPESAFE_API_KEY=... uv run examples/40_judge.py
"""

import os

import superglue

key = os.environ.get("TYPESAFE_API_KEY", "")
if not key:
    print("Set TYPESAFE_API_KEY to run this example.")
    raise SystemExit(0)

config = {
    "id": "example.urgency",
    "model": "typesafe:jev-latest",
    "timeout_ms": 400,
    "on_error": "error",
    "questions": {
        "is_urgent": {
            "type": "noul",
            "instructions": "The message conveys urgency or time-sensitivity",
        }
    },
    "outputs": [
        {
            "kind": "noul",
            "name": "urgent",
            "from": "is_urgent",
            "threshold": 0.5,
            "required": False,
        }
    ],
}

client = superglue.Client(api_key="unused", api_keys={"typesafe": key})
verdict = client.judge(
    config,
    "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. "
    "I'm losing sales. Please help ASAP.",
)
print("urgent:", verdict.get("urgent"))
