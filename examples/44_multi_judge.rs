//! Example 44: three judges on one draft.
//!
//! Set `OPENAI_API_KEY` and `TYPESAFE_API_KEY`.
//! The graph writes a support reply.
//! Safety, tone, and coverage then judge that reply in one wave.
//! Safety chooses a holding reply or a final reply that can use the other verdicts.

mod support;

use serde_json::json;
use superglue::agents::AgentSpec;
use superglue::graph::Graph;
use superglue::judge::JudgeBuilder;
use superglue::providers::ProviderId;
use support::{base_url, model};

const TICKET: &str = "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let openai = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    let typesafe = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
    if openai.is_empty() || typesafe.is_empty() {
        println!("Set OPENAI_API_KEY and TYPESAFE_API_KEY to run this example.");
        return Ok(());
    }

    let model_name = model();
    let writer = AgentSpec::new("desk", "a careful support agent").with_model(model_name.clone());
    let safety = JudgeBuilder::new("desk.safety")
        .noul(
            "safe",
            "The reply does not invent a refund, an account balance, or a delivery date",
        )
        .done()
        .noul_output("pass", "safe")
        .threshold(0.5)
        .done()
        .build()?;
    let tone = JudgeBuilder::new("desk.tone")
        .choice("tone", "Which tone does this reply use?")
        .option("calm", "Polite and specific")
        .option("terse", "Short but not rude")
        .option("rude", "Blames the customer or sounds angry")
        .done()
        .choice_output("label", "tone")
        .map_text("calm", "calm")
        .map_text("terse", "terse")
        .map_text("rude", "rude")
        .emit_confidence()
        .done()
        .build()?;
    let coverage = JudgeBuilder::new("desk.coverage")
        .noul("answers", "The reply addresses the customer's request")
        .done()
        .noul_output("pass", "answers")
        .threshold(0.5)
        .done()
        .build()?;

    let graph = Graph::builder("desk")
        .require("ticket")
        .agent(
            "draft",
            writer.clone(),
            "Write a support reply to this ticket. Use at most 4 sentences. Do not invent a refund or a delivery date.\n{ticket}",
        )
        .judge("safety", safety, "draft")
        .judge("tone", tone, "draft")
        .judge("coverage", coverage, "draft")
        .map(
            "hold",
            "We need a moment to check this ticket. A person will follow up.",
        )
        .agent(
            "reply",
            writer,
            "Ticket:\n{ticket}\n\nDraft:\n{draft}\n\nSafety:\n{safety}\n\nTone:\n{tone}\n\nCoverage:\n{coverage}\n\nWrite the reply the customer should receive. Use at most 4 sentences. If the tone label is rude, rewrite the draft in a calm tone. If coverage.pass is false, add the missing answer. Do not invent a refund or a delivery date.",
        )
        .depends("safety", ["draft"])
        .depends("tone", ["draft"])
        .depends("coverage", ["draft"])
        .depends("reply", ["tone", "coverage"])
        .when("safety", "pass", json!(true), "reply")
        .otherwise("safety", "hold")
        .write("hold", "answer")
        .write("reply", "answer")
        .output("answer")
        .build()?;

    println!("Three judges score one draft. Safety chooses hold or reply.");
    println!("Ticket: {TICKET}");
    println!("Running the graph...");
    let client = superglue::Client::builder()
        .api_key(openai)
        .model(model_name)
        .base_url(base_url())
        .api_key_for(ProviderId::TypeSafe, typesafe)
        .build()?;
    let answer = client.run_graph(&graph, json!({"ticket": TICKET})).await?;
    println!("Answer: {answer}");
    Ok(())
}
