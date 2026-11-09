//! Example 40: a judge config that maps a Noul answer to a bool.
//!
//! The same judge loads from JSON and builds in Rust. Both forms must match.

use superglue::client::ClientBuilder;
use superglue::judge::{JudgeBuilder, JudgeConfig, JudgeInput, OnError};
use superglue::providers::ProviderId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = JudgeConfig::from_json(
        r#"{
            "id": "example.urgency",
            "model": "typesafe:jev-latest",
            "timeout_ms": 400,
            "on_error": "error",
            "questions": {
                "is_urgent": {
                    "type": "noul",
                    "instructions": "The message conveys urgency or time-sensitivity"
                }
            },
            "outputs": [{
                "kind": "noul",
                "name": "urgent",
                "from": "is_urgent",
                "threshold": 0.5,
                "required": false
            }]
        }"#,
    )?;

    let built = JudgeBuilder::new("example.urgency")
        .model("typesafe:jev-latest")
        .timeout_ms(400)
        .on_error(OnError::Error)
        .noul(
            "is_urgent",
            "The message conveys urgency or time-sensitivity",
        )
        .done()
        .noul_output("urgent", "is_urgent")
        .threshold(0.5)
        .done()
        .build()?;
    assert_eq!(built, config);

    let key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
    if key.is_empty() {
        eprintln!("Set TYPESAFE_API_KEY to run this example.");
        return Ok(());
    }

    let client = ClientBuilder::new()
        .api_key("unused")
        .api_key_for(ProviderId::TypeSafe, key)
        .build()?;
    let verdict = client
        .judge(
            &built,
            JudgeInput::text(
                "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
            ),
        )
        .await?;
    println!("urgent: {:?}", verdict.bool("urgent"));
    Ok(())
}
