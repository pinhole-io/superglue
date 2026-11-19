//! Example 45: OpenAI Decisions evaluation with a predicate, a choice, and a score.

use superglue::client::ClientBuilder;
use superglue::decisions::{
    Answer, ChoiceOption, DEFAULT_MODEL, DecisionRequest, Question, ScoreLevel,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if key.trim().is_empty() {
        eprintln!("Set OPENAI_API_KEY to run this example.");
        return Ok(());
    }

    let client = ClientBuilder::new().api_key(key.trim()).build()?;
    let response = client
        .decisions(
            DecisionRequest::new(
                "Hi, I've been trying to connect my Stripe account for 3 days and it keeps failing. I'm losing sales. Please help ASAP.",
                vec![
                    Question::predicate(
                        "is_urgent",
                        "The message conveys urgency or time-sensitivity.",
                    ),
                    Question::choice(
                        "department",
                        "Which team should handle this?",
                        vec![
                            ChoiceOption::new("billing")
                                .description("Payment or subscription issues."),
                            ChoiceOption::new("technical")
                                .description("Bugs or integration problems."),
                            ChoiceOption::new("sales")
                                .description("Pricing or account questions."),
                        ],
                    ),
                    Question::score(
                        "frustration",
                        "How frustrated the customer appears.",
                        vec![
                            ScoreLevel::new("Calm, just stating facts"),
                            ScoreLevel::new("Frustrated but civil"),
                            ScoreLevel::new("Very angry, strong language"),
                        ],
                    ),
                ],
            )
            .with_model(DEFAULT_MODEL),
        )
        .await?;

    println!(
        "department: {:?}",
        response.answer("department").and_then(Answer::choice_text)
    );
    println!(
        "frustration: {:?}",
        response.answer("frustration").and_then(Answer::score)
    );
    println!(
        "is_urgent: {:?}",
        response.answer("is_urgent").and_then(Answer::probability)
    );
    Ok(())
}
