//! Example 42: an advanced judge built in Rust.
//!
//! One call triages a support ticket and checks it against known issues.
//! The judge uses a mapped choice, a Noul, a score, and a repeat template.

use serde_json::json;
use superglue::client::ClientBuilder;
use superglue::judge::{JudgeBuilder, JudgeInput, JudgeItem, OnError};
use superglue::providers::ProviderId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
    if key.is_empty() {
        eprintln!("Set TYPESAFE_API_KEY to run this example.");
        return Ok(());
    }

    let config = JudgeBuilder::new("example.triage")
        .model("typesafe:jev-latest")
        .timeout_ms(400)
        .on_error(OnError::Error)
        .state_chars(240)
        .choice("department", "Which team should handle this ticket?")
        .option("billing", "Payment or subscription issues")
        .option("technical", "Bugs or integration problems")
        .option("sales", "Pricing or account questions")
        .done()
        .noul(
            "is_urgent",
            "The message conveys urgency or time-sensitivity",
        )
        .done()
        .score("frustration", "How frustrated the customer appears")
        .level("Calm, just stating facts")
        .level("Frustrated but civil")
        .level("Very angry, strong language")
        .done()
        .choice_output("team", "department")
        .map_text("billing", "Billing")
        .map_text("technical", "Support")
        .map_text("sales", "Sales")
        // No min_confidence and no required here. A three-way choice can
        // score under 0.5. The example prints team_confidence and lets the
        // caller decide. Add required() when a missing team must fail.
        .emit_confidence()
        .done()
        .noul_output("urgent", "is_urgent")
        .threshold(0.5)
        .done()
        .score_output("frustration", "frustration")
        .done()
        .repeat_noul(
            "matches",
            "Does this ticket match the known issue in the item?",
        )
        .id_prefix("issue:")
        .threshold(0.5)
        .done()
        .build()?;

    let input = JudgeInput::json(json!({
        "ticket": "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
        "issues": [
            { "id": "stripe-connect", "summary": "Stripe account connection fails" },
            { "id": "password-reset", "summary": "Password reset email never arrives" },
            { "id": "invoice-export", "summary": "Invoice export produces an empty file" }
        ]
    }))
    .with_items(vec![
        JudgeItem::new("stripe-connect"),
        JudgeItem::new("password-reset"),
        JudgeItem::new("invoice-export"),
    ]);

    let client = ClientBuilder::new()
        .api_key("unused")
        .api_key_for(ProviderId::TypeSafe, key)
        .build()?;
    let verdict = client.judge(&config, input).await?;

    println!("team: {:?}", verdict.text("team"));
    println!("team confidence: {:?}", verdict.number("team_confidence"));
    println!("urgent: {:?}", verdict.bool("urgent"));
    println!("frustration: {:?}", verdict.number("frustration"));
    println!("matches: {:?}", verdict.map("matches"));
    Ok(())
}
