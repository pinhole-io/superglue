/**
 * Example 30: streaming with registered tool (multi-round SSE).
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient, registerTool } = require("../dist/facade.js");

const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) {
  console.error("Set OPENAI_API_KEY to run this example.");
  process.exit(1);
}

const client = createClient({
  apiKey,
  model: process.env.OPENAI_MODEL ?? "gpt-6-luna",
});

await registerTool(client, {
  name: "echo",
  description: "Echo JSON args",
  parameters: { type: "object" },
  execute: (args) => ({ echo: args }),
});

const tokens = [];
const out = await client.stream(
  "Call echo with {\"msg\":\"hi\"} then say OK",
  (t) => {
    if (t != null && t !== "") {
      tokens.push(t);
      process.stdout.write(t);
    }
  },
  undefined,
  undefined,
  undefined,
  "none",
);
console.log("\nfinish:", out.finishReason);
