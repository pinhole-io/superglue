"""Example 38: file transcription for xAI, OpenAI, or Groq.

The default file is ``examples/sample.mp3``. Pass another path to use that file.
``SUPERGLUE_STT_PROVIDER`` selects ``xai`` (default), ``openai``, or ``groq``.

    XAI_API_KEY=... uv run examples/38_file_transcription.py
"""

import os
import sys
from pathlib import Path

import superglue

KEYS = {
    "xai": "XAI_API_KEY",
    "openai": "OPENAI_API_KEY",
    "groq": "GROQ_API_KEY",
}

provider = os.environ.get("SUPERGLUE_STT_PROVIDER", "xai")
key_var = KEYS.get(provider, "XAI_API_KEY")
if not os.environ.get(key_var, ""):
    print(f"Skip file transcription example (set {key_var})")
    raise SystemExit(0)

if len(sys.argv) > 1:
    path = Path(sys.argv[1])
elif os.environ.get("SUPERGLUE_AUDIO_FILE"):
    path = Path(os.environ["SUPERGLUE_AUDIO_FILE"])
else:
    path = Path(__file__).resolve().parents[2] / "examples" / "sample.mp3"

client = superglue.Client(api_key="unused")
result = client.transcribe(path.name, path.read_bytes(), provider)
print(result["provider"], "model", result["model"])
print(result["text"])
for word in result["words"]:
    print(f"{word['start']:.2f}-{word['end']:.2f} {word['text']}")
