/**
 * Example 36: Realtime voice session with a tool call (OpenAI Realtime).
 *
 *   OPENAI_API_KEY=... node examples/36_realtime_voice.mjs
 *
 * The script sends one text turn and prints the transcript. The assistant
 * transcript arrives as deltas. The script counts the PCM16 audio bytes instead
 * of playing them. OPENAI_REALTIME_MODEL overrides gpt-realtime.
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const key = process.env.OPENAI_API_KEY;
if (!key) {
  console.log("Set OPENAI_API_KEY to run this example.");
  process.exit(0);
}

const client = createClient({ apiKey: key });

let audioBytes = 0;
let reply = "";
let waitingForToolReply = false;
let finish;
const done = new Promise((resolve, reject) => {
  finish = { resolve, reject };
});
const timer = setTimeout(() => finish.reject(new Error("no reply within 60 seconds")), 60_000);

const session = await client.voiceConnect(
  {
    provider: "openai",
    model: process.env.OPENAI_REALTIME_MODEL ?? "gpt-realtime",
    instructions: "Answer in one short sentence.",
    tools: [
      {
        name: "lookup_order",
        description: "Return the status of an order by id.",
        parameters: {
          type: "object",
          properties: { order_id: { type: "string" } },
          required: ["order_id"],
        },
      },
    ],
  },
  (err, { event, audio }) => {
    if (err) return finish.reject(err);
    switch (event.type) {
      case "output_audio":
        audioBytes += audio?.length ?? 0;
        break;
      case "output_transcript":
        reply += event.text;
        break;
      case "function_call": {
        const args = JSON.parse(event.arguments || "{}");
        console.log(`tool call: ${event.name}(${JSON.stringify(args)})`);
        waitingForToolReply = true;
        // This also asks the model for the next response.
        session
          .submitToolResult(event.call_id, { order_id: args.order_id, status: "shipped" })
          .catch(finish.reject);
        break;
      }
      case "response_done":
        if (reply) console.log("assistant:", reply);
        reply = "";
        if (!waitingForToolReply) finish.resolve();
        waitingForToolReply = false;
        break;
      case "error":
        finish.reject(new Error(`voice error: ${event.message}`));
        break;
      case "closed":
        finish.resolve();
        break;
    }
  },
);

await session.sendText("Use lookup_order for order 42 and tell me its status.");
try {
  await done;
} finally {
  clearTimeout(timer);
  await session.close();
}
console.log(`audio: ${audioBytes} bytes of PCM16`);
