/**
 * Example 37: TypeSafe System One evaluation with Noul, Choice, and Score.
 *
 *   TYPESAFE_API_KEY=... node examples/37_systemone.mjs
 */
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { createClient } = require("../dist/facade.js");

const key = process.env.TYPESAFE_API_KEY;
if (!key) {
  console.log("Set TYPESAFE_API_KEY to run this example.");
  process.exit(0);
}

const client = createClient({
  apiKey: "unused",
  apiKeys: { typesafe: key },
});

if (typeof client.systemOne !== "function") {
  console.error("Client.systemOne is missing. Rebuild superglue-js from the current source.");
  process.exit(1);
}

const response = await client.systemOne(
  "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
  {
    is_urgent: {
      type: "noul",
      instructions: "The message conveys urgency or time-sensitivity",
    },
    department: {
      type: "choice",
      instructions: "Which team should handle this",
      criteria: {
        billing: "Payment or subscription issues",
        technical: "Bugs or integration problems",
        sales: "Pricing or account questions",
      },
    },
    frustration: {
      type: "score",
      instructions: "How frustrated the customer appears",
      criteria: [
        "Calm, just stating facts",
        "Frustrated but civil",
        "Very angry, strong language",
      ],
    },
  },
);

const answers = response.answers;
console.log("department:", answers.department?.choice);
console.log("frustration:", answers.frustration?.score);
console.log("is_urgent:", answers.is_urgent?.noul);
