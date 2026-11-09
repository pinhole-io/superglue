/**
 * Example 43: a graph that drafts, then polishes.
 *
 * Set OPENAI_API_KEY. The graph writes a short draft, then polishes it.
 * Set TYPESAFE_API_KEY to judge the draft before the polish step.
 *
 *   OPENAI_API_KEY=... node examples/43_graph.mjs
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const apiKey = process.env.OPENAI_API_KEY ?? "";
if (!apiKey) {
  console.error("Set OPENAI_API_KEY to run this example.");
  process.exit(1);
}

const model = process.env.OPENAI_MODEL ?? "gpt-6-luna";
const baseUrl =
  process.env.OPENAI_BASE_URL ??
  (apiKey.startsWith("sk-") ? "https://api.openai.com" : "http://localhost:11434");
const typesafe = process.env.TYPESAFE_API_KEY ?? "";

function writer() {
  return {
    name: "writer",
    persona: "a careful writer",
    goals: [],
    instructions: [],
    constraints: [],
    model,
    max_tool_rounds: 16,
    max_output_retries: 3,
    system_prompt_override: null,
    reasoning_effort: null,
  };
}

function draftNode() {
  return {
    id: "draft",
    visits: 1,
    step: {
      kind: "agent",
      spec: writer(),
      prompt: "Write a short draft about {question}. Use at most 4 sentences.",
    },
  };
}

function polishNode(write) {
  const node = {
    id: "polish",
    visits: 1,
    step: {
      kind: "agent",
      spec: writer(),
      prompt: "Polish this draft. Keep it under 4 sentences.\n{draft}",
    },
  };
  if (write === undefined) {
    node.depends = ["draft"];
  } else {
    node.write = write;
  }
  return node;
}

const spec = typesafe
  ? {
      name: "review",
      require: ["question"],
      output: "answer",
      nodes: [
        draftNode(),
        {
          id: "grade",
          visits: 1,
          depends: ["draft"],
          step: {
            kind: "judge",
            input: "draft",
            config: {
              id: "review.pass",
              model: "typesafe:jev-latest",
              timeout_ms: 400,
              on_error: "error",
              questions: {
                good: {
                  type: "noul",
                  instructions: "The draft answers the question in clear prose",
                },
              },
              outputs: [
                {
                  kind: "noul",
                  name: "pass",
                  from: "good",
                  threshold: 0.5,
                  required: false,
                },
              ],
            },
          },
          when: [{ path: "pass", equals: true, target: "polish" }],
          otherwise: "keep",
        },
        polishNode("answer"),
        {
          id: "keep",
          write: "answer",
          visits: 1,
          step: { kind: "map", template: "{draft}" },
        },
      ],
    }
  : {
      name: "review",
      require: ["question"],
      output: "polish",
      nodes: [draftNode(), polishNode()],
    };

const client = createClient({
  apiKey,
  model,
  baseUrl,
  apiKeys: typesafe ? { typesafe } : undefined,
});

if (typeof client.runGraph !== "function") {
  console.error("Client.runGraph is missing. Rebuild superglue-js from the current source.");
  process.exit(1);
}

if (typesafe) {
  console.log("The graph drafts, judges, then polishes or keeps the draft.");
} else {
  console.log("TYPESAFE_API_KEY is unset. The graph drafts, then polishes.");
}
console.log("Question: Rust ownership");
console.log("Running the graph...");
const answer = await client.runGraph(spec, { question: "Rust ownership" });
console.log("Answer:", answer);
