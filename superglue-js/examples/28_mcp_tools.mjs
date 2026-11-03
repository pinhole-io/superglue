/**
 * Example 28: MCP stdio tools (set MCP_RUN=1).
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

if (process.env.MCP_RUN !== "1") {
  console.log("Skip MCP example (set MCP_RUN=1 and ensure npx is available)");
  process.exit(0);
}

const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) {
  console.error("Set OPENAI_API_KEY to run this example.");
  process.exit(1);
}

const client = createClient({
  apiKey,
  model: process.env.OPENAI_MODEL ?? "gpt-6-luna",
});

await client.connectMcpStdio(
  "npx",
  ["-y", "@modelcontextprotocol/server-everything"],
  null,
  "mcp",
);

const result = await client.complete(
  "Name one available tool with the mcp__ prefix. One line only.",
  undefined,
  undefined,
  undefined,
  "none",
);
console.log("content:", result.content);
