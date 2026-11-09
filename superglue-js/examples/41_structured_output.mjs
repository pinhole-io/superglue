/**
 * Example 41: a completion that returns one JSON object checked against a schema.
 *
 *   OPENAI_API_KEY=... node examples/41_structured_output.mjs
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const apiKey = process.env.OPENAI_API_KEY ?? "";
if (!apiKey) {
  console.log("Set OPENAI_API_KEY to run this example.");
  process.exit(0);
}

const baseUrl =
  process.env.OPENAI_BASE_URL ??
  (apiKey.startsWith("sk-") ? "https://api.openai.com" : "http://localhost:11434");
const client = createClient({
  apiKey,
  model: process.env.OPENAI_MODEL ?? "gpt-6-luna",
  baseUrl,
});
const person = await client.completeObject(
  "Ada Lovelace, born in 1815, was an English mathematician.",
  {
    name: "person",
    description: "A person mentioned in the text.",
    schema: {
      type: "object",
      properties: {
        name: { type: "string" },
        born: { type: "integer" },
        occupation: { type: "string" },
      },
      required: ["name", "born", "occupation"],
      additionalProperties: false,
    },
  },
);
console.log("name:", person.name);
console.log("born:", person.born);
console.log("occupation:", person.occupation);
