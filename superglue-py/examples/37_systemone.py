"""Example 37: TypeSafe System One evaluation with Noul, Choice, and Score.

    TYPESAFE_API_KEY=... uv run examples/37_systemone.py
"""

import os

import superglue

key = os.environ.get("TYPESAFE_API_KEY", "")
if not key:
    print("Set TYPESAFE_API_KEY to run this example.")
    raise SystemExit(0)

client = superglue.Client(
    api_key="unused",
    api_keys={"typesafe": key},
)

state = (
    "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. "
    "I'm losing sales. Please help ASAP."
)
response = client.system_one(
    state,
    {
        "is_urgent": {
            "type": "noul",
            "instructions": "The message conveys urgency or time-sensitivity",
        },
        "department": {
            "type": "choice",
            "instructions": "Which team should handle this",
            "criteria": {
                "billing": "Payment or subscription issues",
                "technical": "Bugs or integration problems",
                "sales": "Pricing or account questions",
            },
        },
        "frustration": {
            "type": "score",
            "instructions": "How frustrated the customer appears",
            "criteria": [
                "Calm, just stating facts",
                "Frustrated but civil",
                "Very angry, strong language",
            ],
        },
    },
)

answers = response["answers"]
print("department:", answers["department"].get("choice"))
print("frustration:", answers["frustration"].get("score"))
print("is_urgent:", answers["is_urgent"].get("noul"))
