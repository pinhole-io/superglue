//! OpenAI Decisions client.
//!
//! Decisions is an evaluation API, like TypeSafe System One. It answers a
//! predicate, a choice, or a score. It does not generate chat text.

mod client;
mod types;

pub use client::{
    DECISIONS_PATH, DecisionError, decisions, qualify_model, request_from_parts, wire_model,
};
pub use types::{
    Answer, ChoiceOption, ChoiceValue, DEFAULT_MODEL, DecisionRequest, DecisionResponse,
    DecisionUsage, Input, MAX_QUESTIONS, Probability, Question, SCORE_LEVELS, ScoreLevel,
};

#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn serializes_a_choice_question() {
        let request = DecisionRequest::new(
            "I was charged twice.",
            vec![Question::choice(
                "department",
                "Which department should handle this complaint?",
                vec![
                    ChoiceOption::new("billing").description("Payments and refunds."),
                    ChoiceOption::new("technical").description("Product problems."),
                ],
            )],
        )
        .with_model(DEFAULT_MODEL);
        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["model"], DEFAULT_MODEL);
        assert_eq!(json["input"], "I was charged twice.");
        assert_eq!(json["questions"][0]["type"], "choice");
        assert_eq!(json["questions"][0]["choices"][0]["value"], "billing");
    }

    #[test]
    fn deserializes_choice_predicate_and_refusal() {
        let json = serde_json::json!({
            "model": "gpt-6-luna",
            "answers": [
                {
                    "type": "choice",
                    "name": "department",
                    "choice": "billing",
                    "confidence": 0.93,
                    "probabilities": [
                        { "value": "billing", "probability": 0.95 },
                        { "value": "technical", "probability": 0.05 }
                    ]
                },
                { "type": "predicate", "name": "refund", "probability": 0.8 },
                { "type": "refusal", "name": "blocked" }
            ],
            "usage": { "input_tokens": 42 }
        });
        let response: DecisionResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(
            response.answer("department").and_then(Answer::choice_text),
            Some("billing")
        );
        assert_eq!(
            response.answer("refund").and_then(Answer::probability),
            Some(0.8)
        );
        assert!(response.answer("blocked").is_some_and(Answer::is_refusal));
        assert_eq!(response.usage.input_tokens, Some(42));
        assert_eq!(response.usage.to_proto().prompt_tokens, 42);
    }

    #[test]
    fn deserializes_a_score_level_index() {
        let json = serde_json::json!({
            "model": "gpt-6-luna",
            "answers": [{
                "type": "score",
                "name": "severity",
                "score": 1.1,
                "confidence": 0.55,
                "probabilities": [
                    { "value": 0, "label": "Cosmetic", "probability": 0.1 },
                    { "value": 1, "label": "Workaround available", "probability": 0.7 },
                    { "value": 2, "label": "Fully blocked", "probability": 0.2 }
                ]
            }]
        });
        let response: DecisionResponse = serde_json::from_value(json).expect("parse");
        let answer = response.answer("severity").expect("severity");
        assert_eq!(answer.score(), Some(1.1));
        let Answer::Score { probabilities, .. } = answer else {
            panic!("severity is a score");
        };
        assert_eq!(probabilities[0].value, ChoiceValue::Index(0));
        assert_eq!(probabilities[0].label.as_deref(), Some("Cosmetic"));
    }
}
