/**
 * Example 40: a judge config that maps a Noul answer to a bool.
 *
 *   TYPESAFE_API_KEY=... node examples/40_judge.mjs
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const key = process.env.TYPESAFE_API_KEY ?? "";
if (!key) {
  console.log("Set TYPESAFE_API_KEY to run this example.");
  process.exit(0);
}

const client = createClient({
  apiKey: "unused",
  apiKeys: { typesafe: key },
});
const verdict = await client.judge(
  {
    id: "example.urgency",
    model: "typesafe:jev-latest",
    timeout_ms: 400,
    on_error: "error",
    questions: {
      is_urgent: {
        type: "noul",
        instructions: "The message conveys urgency or time-sensitivity",
      },
    },
    outputs: [
      {
        kind: "noul",
        name: "urgent",
        from: "is_urgent",
        threshold: 0.5,
        required: false,
      },
    ],
  },
  "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
);
console.log("urgent:", verdict.urgent);
