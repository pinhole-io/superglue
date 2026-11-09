//! Example 43: a graph that drafts, then polishes.
//!
//! Set `OPENAI_API_KEY`. The graph writes a short draft, then polishes it.
//! Set `TYPESAFE_API_KEY` to judge the draft before the polish step.

mod support;

use serde_json::json;
use superglue::agents::AgentSpec;
use superglue::graph::Graph;
use superglue::judge::JudgeBuilder;
use superglue::providers::ProviderId;
use support::{base_url, model, require_api_key};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let openai = require_api_key();
    let model_name = model();
    let typesafe = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
    let writer = AgentSpec::new("writer", "a careful writer").with_model(model_name.clone());

    let mut client = superglue::Client::builder()
        .api_key(openai)
        .model(model_name)
        .base_url(base_url());

    let graph = if typesafe.is_empty() {
        println!("TYPESAFE_API_KEY is unset. The graph drafts, then polishes.");
        Graph::builder("review")
            .require("question")
            .agent(
                "draft",
                writer.clone(),
                "Write a short draft about {question}. Use at most 4 sentences.",
            )
            .agent(
                "polish",
                writer,
                "Polish this draft. Keep it under 4 sentences.\n{draft}",
            )
            .depends("polish", ["draft"])
            .output("polish")
            .build()?
    } else {
        println!("The graph drafts, judges, then polishes or keeps the draft.");
        client = client.api_key_for(ProviderId::TypeSafe, typesafe);
        let judge = JudgeBuilder::new("review.pass")
            .noul("good", "The draft answers the question in clear prose")
            .done()
            .noul_output("pass", "good")
            .threshold(0.5)
            .done()
            .build()?;
        Graph::builder("review")
            .require("question")
            .agent(
                "draft",
                writer.clone(),
                "Write a short draft about {question}. Use at most 4 sentences.",
            )
            .judge("grade", judge, "draft")
            .agent(
                "polish",
                writer,
                "Polish this draft. Keep it under 4 sentences.\n{draft}",
            )
            .map("keep", "{draft}")
            .depends("grade", ["draft"])
            .when("grade", "pass", json!(true), "polish")
            .otherwise("grade", "keep")
            .write("polish", "answer")
            .write("keep", "answer")
            .output("answer")
            .build()?
    };

    println!("Question: Rust ownership");
    println!("Running the graph...");
    let answer = client
        .build()?
        .run_graph(&graph, json!({"question": "Rust ownership"}))
        .await?;
    println!("Answer: {answer}");
    Ok(())
}
