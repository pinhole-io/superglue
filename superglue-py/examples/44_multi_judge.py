"""Example 44: three judges on one draft.

Set ``OPENAI_API_KEY`` and ``TYPESAFE_API_KEY``.
The graph writes a support reply.
Safety, tone, and coverage then judge that reply in one wave.
Safety chooses a holding reply or a final reply that can use the other verdicts.

    OPENAI_API_KEY=... TYPESAFE_API_KEY=... uv run examples/44_multi_judge.py
"""

import os

import superglue

TICKET = (
    "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. "
    "I'm losing sales. Please help ASAP."
)

openai = os.environ.get("OPENAI_API_KEY", "")
typesafe = os.environ.get("TYPESAFE_API_KEY", "")
if not openai or not typesafe:
    print("Set OPENAI_API_KEY and TYPESAFE_API_KEY to run this example.")
    raise SystemExit(0)

model = os.environ.get("OPENAI_MODEL", "gpt-6-luna")
base_url = os.environ.get("OPENAI_BASE_URL")
if base_url is None:
    base_url = (
        "https://api.openai.com" if openai.startswith("sk-") else "http://localhost:11434"
    )


def writer():
    return {
        "name": "desk",
        "persona": "a careful support agent",
        "goals": [],
        "instructions": [],
        "constraints": [],
        "model": model,
        "max_tool_rounds": 16,
        "max_output_retries": 3,
        "system_prompt_override": None,
        "reasoning_effort": None,
    }


spec = {
    "name": "desk",
    "require": ["ticket"],
    "output": "answer",
    "nodes": [
        {
            "id": "draft",
            "step": {
                "kind": "agent",
                "spec": writer(),
                "prompt": (
                    "Write a support reply to this ticket. Use at most 4 sentences. "
                    "Do not invent a refund or a delivery date.\n{ticket}"
                ),
            },
        },
        {
            "id": "safety",
            "depends": ["draft"],
            "step": {
                "kind": "judge",
                "input": "draft",
                "config": {
                    "id": "desk.safety",
                    "questions": {
                        "safe": {
                            "type": "noul",
                            "instructions": (
                                "The reply does not invent a refund, an account balance, "
                                "or a delivery date"
                            ),
                        }
                    },
                    "outputs": [
                        {"kind": "noul", "name": "pass", "from": "safe", "threshold": 0.5}
                    ],
                },
            },
            "when": [{"path": "pass", "equals": True, "target": "reply"}],
            "otherwise": "hold",
        },
        {
            "id": "tone",
            "depends": ["draft"],
            "step": {
                "kind": "judge",
                "input": "draft",
                "config": {
                    "id": "desk.tone",
                    "questions": {
                        "tone": {
                            "type": "choice",
                            "instructions": "Which tone does this reply use?",
                            "options": {
                                "calm": "Polite and specific",
                                "terse": "Short but not rude",
                                "rude": "Blames the customer or sounds angry",
                            },
                        }
                    },
                    "outputs": [
                        {
                            "kind": "choice",
                            "name": "label",
                            "from": "tone",
                            "map": {"calm": "calm", "terse": "terse", "rude": "rude"},
                            "emit_confidence": True,
                        }
                    ],
                },
            },
        },
        {
            "id": "coverage",
            "depends": ["draft"],
            "step": {
                "kind": "judge",
                "input": "draft",
                "config": {
                    "id": "desk.coverage",
                    "questions": {
                        "answers": {
                            "type": "noul",
                            "instructions": "The reply addresses the customer's request",
                        }
                    },
                    "outputs": [
                        {
                            "kind": "noul",
                            "name": "pass",
                            "from": "answers",
                            "threshold": 0.5,
                        }
                    ],
                },
            },
        },
        {
            "id": "hold",
            "write": "answer",
            "step": {
                "kind": "map",
                "template": "We need a moment to check this ticket. A person will follow up.",
            },
        },
        {
            "id": "reply",
            "write": "answer",
            "depends": ["tone", "coverage"],
            "step": {
                "kind": "agent",
                "spec": writer(),
                "prompt": (
                    "Ticket:\n{ticket}\n\nDraft:\n{draft}\n\nSafety:\n{safety}\n\n"
                    "Tone:\n{tone}\n\nCoverage:\n{coverage}\n\n"
                    "Write the reply the customer should receive. Use at most 4 sentences. "
                    "If the tone label is rude, rewrite the draft in a calm tone. "
                    "If coverage.pass is false, add the missing answer. "
                    "Do not invent a refund or a delivery date."
                ),
            },
        },
    ],
}

print("Three judges score one draft. Safety chooses hold or reply.")
print("Ticket:", TICKET)
print("Running the graph...")
client = superglue.Client(
    api_key=openai,
    model=model,
    base_url=base_url,
    api_keys={"typesafe": typesafe},
)
answer = client.run_graph(spec, {"ticket": TICKET})
print("Answer:", answer)
