/**
 * Example 34: Live context-optimization comparison.
 *
 * Runs one prompt on a standard tool client and on a dynamic, condensed client.
 * Prints rounds and token usage for each.
 *
 * The Rust example also runs the fat-payload and multi-turn matrix.
 * Set SUPERGLUE_BENCH_ONLY there to select a section.
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient, registerTool } = require("../dist/facade.js");

const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) {
  console.error("Set OPENAI_API_KEY to run this example.");
  process.exit(1);
}

const model = process.env.OPENAI_MODEL ?? "gpt-6-luna";
const prompt = "What's the weather in Paris?";

async function registerContextTools(client) {
  await registerTool(client, {
    name: "get_weather",
    description: "Get weather for a city",
    parameters: {
      type: "object",
      properties: { city: { type: "string" } },
      required: ["city"],
    },
    execute: ({ city }) => ({ city, temp_f: 72, condition: "sunny" }),
  });
  await registerTool(client, {
    name: "calculate",
    description: "Evaluate a math expression",
    parameters: {
      type: "object",
      properties: { expression: { type: "string" } },
      required: ["expression"],
    },
    execute: ({ expression }) => ({ expression, result: 42 }),
  });
  await registerTool(client, {
    name: "get_time",
    description: "Return current UTC time (pinned static tool)",
    parameters: { type: "object", properties: {} },
    execute: () => ({ time: "12:00Z" }),
    staticTool: true,
  });
}

async function run(label, options) {
  const client = createClient({
    apiKey,
    model,
    systemPrompt: "You are a helpful assistant. Use tools when needed.",
    ...options,
  });
  await registerContextTools(client);
  const result = await client.complete(
    prompt,
    undefined,
    undefined,
    undefined,
    "none",
  );
  console.log(`${label} rounds:`, result.rounds);
  console.log(`${label} usage:`, result.usage);
  console.log(`${label} content:`, result.content);
}

await run("standard", {});
await run("optimized", { toolMode: "dynamic", condenseToolMessages: true });
