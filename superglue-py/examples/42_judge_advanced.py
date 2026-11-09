"""Example 42: one judge call triages a ticket and checks known issues.

    TYPESAFE_API_KEY=... uv run examples/42_judge_advanced.py
"""

import os

import superglue

key = os.environ.get("TYPESAFE_API_KEY", "")
if not key:
    print("Set TYPESAFE_API_KEY to run this example.")
    raise SystemExit(0)

config = {
    "id": "example.triage",
    "model": "typesafe:jev-latest",
    "timeout_ms": 400,
    "state_chars": 240,
    "on_error": "error",
    "questions": {
        "department": {
            "type": "choice",
            "instructions": "Which team should handle this ticket?",
            "options": {
                "billing": "Payment or subscription issues",
                "sales": "Pricing or account questions",
                "technical": "Bugs or integration problems",
            },
        },
        "frustration": {
            "type": "score",
            "instructions": "How frustrated the customer appears",
            "levels": [
                "Calm, just stating facts",
                "Frustrated but civil",
                "Very angry, strong language",
            ],
        },
        "is_urgent": {
            "type": "noul",
            "instructions": "The message conveys urgency or time-sensitivity",
        },
    },
    "outputs": [
        {
            "kind": "choice",
            "name": "team",
            "from": "department",
            "map": {
                "billing": "Billing",
                "sales": "Sales",
                "technical": "Support",
            },
            "min_confidence": 0,
            "on_unknown": "error",
            "required": False,
            "emit_confidence": True,
        },
        {
            "kind": "noul",
            "name": "urgent",
            "from": "is_urgent",
            "threshold": 0.5,
            "required": False,
        },
        {
            "kind": "score",
            "name": "frustration",
            "from": "frustration",
            "required": False,
        },
    ],
    "repeat": {
        "id_prefix": "issue:",
        "template": {
            "type": "noul",
            "instructions": "Does this ticket match the known issue in the item?",
        },
        "output": {
            "kind": "noul",
            "name": "matches",
            "threshold": 0.5,
        },
    },
}
state = {
    "ticket": (
        "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. "
        "I'm losing sales. Please help ASAP."
    ),
    "issues": [
        {"id": "stripe-connect", "summary": "Stripe account connection fails"},
        {"id": "password-reset", "summary": "Password reset email never arrives"},
        {"id": "invoice-export", "summary": "Invoice export produces an empty file"},
    ],
}
items = [
    {"id": "stripe-connect"},
    {"id": "password-reset"},
    {"id": "invoice-export"},
]

client = superglue.Client(api_key="unused", api_keys={"typesafe": key})
verdict = client.judge(config, state, items)
print("team:", verdict.get("team"))
print("team confidence:", verdict.get("team_confidence"))
print("urgent:", verdict.get("urgent"))
print("frustration:", verdict.get("frustration"))
print("matches:", verdict.get("matches"))
