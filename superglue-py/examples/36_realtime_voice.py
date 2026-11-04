"""Example 36: Realtime voice session with a tool call (OpenAI Realtime).

    OPENAI_API_KEY=... uv run examples/36_realtime_voice.py

The script sends one text turn and prints the transcript. The assistant
transcript arrives as deltas. The script counts the PCM16 audio bytes instead
of playing them. ``OPENAI_REALTIME_MODEL`` overrides
``gpt-realtime``.
"""

import json
import os
import queue

import superglue

key = os.environ.get("OPENAI_API_KEY", "")
if not key:
    print("Set OPENAI_API_KEY to run this example.")
    raise SystemExit(0)

client = superglue.Client(api_key=key)
events: queue.Queue[dict] = queue.Queue()

# The callback runs on a native thread. Hand each event to the main thread.
session = client.voice_connect(
    {
        "provider": "openai",
        "model": os.environ.get("OPENAI_REALTIME_MODEL", "gpt-realtime"),
        "instructions": "Answer in one short sentence.",
        "tools": [
            {
                "name": "lookup_order",
                "description": "Return the status of an order by id.",
                "parameters": {
                    "type": "object",
                    "properties": {"order_id": {"type": "string"}},
                    "required": ["order_id"],
                },
            }
        ],
    },
    on_event=events.put,
)

session.send_text("Use lookup_order for order 42 and tell me its status.")

audio_bytes = 0
reply: list[str] = []
waiting_for_tool_reply = False
try:
    while True:
        event = events.get(timeout=60)
        kind = event["type"]
        if kind == "output_audio":
            audio_bytes += len(event["audio"])
        elif kind == "output_transcript":
            reply.append(event["text"])
        elif kind == "function_call":
            args = json.loads(event["arguments"] or "{}")
            print(f"tool call: {event['name']}({args})")
            # This also asks the model for the next response.
            session.submit_tool_result(
                event["call_id"], {"order_id": args.get("order_id"), "status": "shipped"}
            )
            waiting_for_tool_reply = True
        elif kind == "response_done":
            if reply:
                print("assistant:", "".join(reply))
                reply.clear()
            if not waiting_for_tool_reply:
                break
            waiting_for_tool_reply = False
        elif kind == "error":
            raise SystemExit(f"voice error: {event['message']}")
        elif kind == "closed":
            break
except queue.Empty:
    raise SystemExit("no voice event for 60 seconds") from None
finally:
    session.close()

print(f"audio: {audio_bytes} bytes of PCM16")
