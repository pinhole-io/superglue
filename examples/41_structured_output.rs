//! Example 41: a completion that returns one JSON object checked against a schema.

mod support;

use serde_json::json;
use superglue::CallOptions;
use superglue::openai::JsonSchemaFormat;
use support::{client_from_env, usage_line};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("Set OPENAI_API_KEY to run this example.");
        return Ok(());
    }

    let client = client_from_env()?;
    let schema = JsonSchemaFormat {
        name: "person".into(),
        description: Some("A person mentioned in the text.".into()),
        schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "born": { "type": "integer" },
                "occupation": { "type": "string" }
            },
            "required": ["name", "born", "occupation"],
            "additionalProperties": false
        }),
        strict: None,
    };

    let object = client
        .complete_object(
            "Ada Lovelace, born in 1815, was an English mathematician.",
            schema,
            CallOptions::default(),
        )
        .await?;

    println!("value: {}", serde_json::to_string_pretty(&object.value)?);
    println!("rounds: {}", object.outcome.rounds);
    println!("{}", usage_line(&object.outcome));
    Ok(())
}
