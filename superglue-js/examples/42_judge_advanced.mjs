/**
 * Example 42: one judge call triages a ticket and checks known issues.
 *
 *   TYPESAFE_API_KEY=... node examples/42_judge_advanced.mjs
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
    id: "example.triage",
    state_chars: 240,
    on_error: "error",
    questions: {
      department: {
        type: "choice",
        instructions: "Which team should handle this ticket?",
        options: {
          billing: "Payment or subscription issues",
          sales: "Pricing or account questions",
          technical: "Bugs or integration problems",
        },
      },
      frustration: {
        type: "score",
        instructions: "How frustrated the customer appears",
        levels: [
          "Calm, just stating facts",
          "Frustrated but civil",
          "Very angry, strong language",
        ],
      },
      is_urgent: {
        type: "noul",
        instructions: "The message conveys urgency or time-sensitivity",
      },
    },
    outputs: [
      {
        kind: "choice",
        name: "team",
        from: "department",
        map: { billing: "Billing", sales: "Sales", technical: "Support" },
        emit_confidence: true,
      },
      { kind: "noul", name: "urgent", from: "is_urgent", threshold: 0.5 },
      { kind: "score", name: "frustration", from: "frustration" },
    ],
    repeat: {
      id_prefix: "issue:",
      template: {
        type: "noul",
        instructions: "Does this ticket match the known issue in the item?",
      },
      output: { kind: "noul", name: "matches", threshold: 0.5 },
    },
  },
  {
    ticket:
      "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
    issues: [
      { id: "stripe-connect", summary: "Stripe account connection fails" },
      { id: "password-reset", summary: "Password reset email never arrives" },
      { id: "invoice-export", summary: "Invoice export produces an empty file" },
    ],
  },
  [
    { id: "stripe-connect" },
    { id: "password-reset" },
    { id: "invoice-export" },
  ],
);
console.log("team:", verdict.team);
console.log("team confidence:", verdict.team_confidence);
console.log("urgent:", verdict.urgent);
console.log("frustration:", verdict.frustration);
console.log("matches:", verdict.matches);
