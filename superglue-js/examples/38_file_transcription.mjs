/**
 * Example 38: file transcription for xAI, OpenAI, or Groq.
 *
 * The default file is `examples/sample.mp3`. Pass another path to use that file.
 * SUPERGLUE_STT_PROVIDER selects `xai` (default), `openai`, or `groq`.
 *
 *   XAI_API_KEY=... node examples/38_file_transcription.mjs
 */
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const fs = require("node:fs");
const path = require("node:path");
const { createClient } = require("../dist/facade.js");

const keys = {
  xai: "XAI_API_KEY",
  openai: "OPENAI_API_KEY",
  groq: "GROQ_API_KEY",
};
const provider = process.env.SUPERGLUE_STT_PROVIDER ?? "xai";
const keyVar = keys[provider] ?? "XAI_API_KEY";
if (!process.env[keyVar]) {
  console.log(`Skip file transcription example (set ${keyVar})`);
  process.exit(0);
}

const here = path.dirname(fileURLToPath(import.meta.url));
const audioPath =
  process.argv[2] ??
  process.env.SUPERGLUE_AUDIO_FILE ??
  path.join(here, "../../examples/sample.mp3");
const client = createClient({ apiKey: "unused" });
if (typeof client.transcribe !== "function") {
  console.error("Client.transcribe is missing. Rebuild superglue-js from the current source.");
  process.exit(1);
}

const result = await client.transcribe(path.basename(audioPath), fs.readFileSync(audioPath), provider);
console.log(result.provider, "model", result.model);
console.log(result.text);
for (const word of result.words) {
  console.log(`${Number(word.start).toFixed(2)}-${Number(word.end).toFixed(2)} ${word.text}`);
}
