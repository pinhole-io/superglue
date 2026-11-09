/**
 * Example 44: three judges on one draft.
 *
 * Set OPENAI_API_KEY and TYPESAFE_API_KEY.
 * The graph writes a support reply.
 * Safety, tone, and coverage then judge that reply in one wave.
 * Safety chooses a holding reply or a final reply that can use the other verdicts.
 *
 *   OPENAI_API_KEY=... TYPESAFE_API_KEY=... node examples/44_multi_judge.mjs
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const TICKET =
  "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.";

const openai = process.env.OPENAI_API_KEY ?? "";
const typesafe = process.env.TYPESAFE_API_KEY ?? "";
if (!openai || !typesafe) {
  console.log("Set OPENAI_API_KEY and TYPESAFE_API_KEY to run this example.");
  process.exit(0);
}

const model = process.env.OPENAI_MODEL ?? "gpt-6-luna";
const baseUrl =
  process.env.OPENAI_BASE_URL ??
  (openai.startsWith("sk-") ? "https://api.openai.com" : "http://localhost:11434");

function writer() {
  return {
    name: "desk",
    persona: "a careful support agent",
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

const spec = {
  name: "desk",
  require: ["ticket"],
  output: "answer",
  nodes: [
    {
      id: "draft",
      visits: 1,
      step: {
        kind: "agent",
        spec: writer(),
        prompt:
          "Write a support reply to this ticket. Use at most 4 sentences. Do not invent a refund or a delivery date.\n{ticket}",
      },
    },
    {
      id: "safety",
      visits: 1,
      depends: ["draft"],
      step: {
        kind: "judge",
        input: "draft",
        config: {
          id: "desk.safety",
          model: "typesafe:jev-latest",
          timeout_ms: 400,
          on_error: "error",
          questions: {
            safe: {
              type: "noul",
              instructions:
                "The reply does not invent a refund, an account balance, or a delivery date",
            },
          },
          outputs: [
            {
              kind: "noul",
              name: "pass",
              from: "safe",
              threshold: 0.5,
              required: false,
            },
          ],
        },
      },
      when: [{ path: "pass", equals: true, target: "reply" }],
      otherwise: "hold",
    },
    {
      id: "tone",
      visits: 1,
      depends: ["draft"],
      step: {
        kind: "judge",
        input: "draft",
        config: {
          id: "desk.tone",
          model: "typesafe:jev-latest",
          timeout_ms: 400,
          on_error: "error",
          questions: {
            tone: {
              type: "choice",
              instructions: "Which tone does this reply use?",
              options: {
                calm: "Polite and specific",
                terse: "Short but not rude",
                rude: "Blames the customer or sounds angry",
              },
            },
          },
          outputs: [
            {
              kind: "choice",
              name: "label",
              from: "tone",
              map: { calm: "calm", terse: "terse", rude: "rude" },
              min_confidence: 0,
              on_unknown: "error",
              required: false,
              emit_confidence: true,
            },
          ],
        },
      },
    },
    {
      id: "coverage",
      visits: 1,
      depends: ["draft"],
      step: {
        kind: "judge",
        input: "draft",
        config: {
          id: "desk.coverage",
          model: "typesafe:jev-latest",
          timeout_ms: 400,
          on_error: "error",
          questions: {
            answers: {
              type: "noul",
              instructions: "The reply addresses the customer's request",
            },
          },
          outputs: [
            {
              kind: "noul",
              name: "pass",
              from: "answers",
              threshold: 0.5,
              required: false,
            },
          ],
        },
      },
    },
    {
      id: "hold",
      write: "answer",
      visits: 1,
      step: {
        kind: "map",
        template: "We need a moment to check this ticket. A person will follow up.",
      },
    },
    {
      id: "reply",
      write: "answer",
      visits: 1,
      depends: ["tone", "coverage"],
      step: {
        kind: "agent",
        spec: writer(),
        prompt:
          "Ticket:\n{ticket}\n\nDraft:\n{draft}\n\nSafety:\n{safety}\n\nTone:\n{tone}\n\nCoverage:\n{coverage}\n\nWrite the reply the customer should receive. Use at most 4 sentences. If the tone label is rude, rewrite the draft in a calm tone. If coverage.pass is false, add the missing answer. Do not invent a refund or a delivery date.",
      },
    },
  ],
};

const client = createClient({
  apiKey: openai,
  model,
  baseUrl,
  apiKeys: { typesafe },
});
if (typeof client.runGraph !== "function") {
  console.error("Client.runGraph is missing. Rebuild superglue-js from the current source.");
  process.exit(1);
}

console.log("Three judges score one draft. Safety chooses hold or reply.");
console.log("Ticket:", TICKET);
console.log("Running the graph...");
const answer = await client.runGraph(spec, { ticket: TICKET });
console.log("Answer:", answer);
