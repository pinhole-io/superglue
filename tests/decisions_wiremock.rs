//! OpenAI Decisions client tests against Wiremock.

use serde_json::json;
use superglue::client::Client;
use superglue::decisions::{ChoiceOption, DecisionError, DecisionRequest, Question, decisions};
use superglue::http::{ClientConfig, HttpClient, RetryPolicy};
use superglue::providers::{ProviderCredentials, ProviderId};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn success_body() -> serde_json::Value {
    json!({
        "model": "gpt-6-luna",
        "answers": [{
            "type": "choice",
            "name": "action",
            "choice": "bet",
            "confidence": 0.7,
            "probabilities": [
                { "value": "pass", "probability": 0.3 },
                { "value": "bet", "probability": 0.7 }
            ]
        }],
        "usage": { "input_tokens": 20, "output_tokens": 4 }
    })
}

fn action_request() -> DecisionRequest {
    DecisionRequest::new(
        "Hero holds 3. The history is empty.",
        vec![Question::choice(
            "action",
            "Which action do you take?",
            vec![ChoiceOption::new("pass"), ChoiceOption::new("bet")],
        )],
    )
}

fn openai_creds(base_url: &str) -> ProviderCredentials {
    let mut creds = ProviderCredentials::new();
    creds.insert_key(ProviderId::OpenAi, "sk-test");
    creds.insert_base_url(ProviderId::OpenAi, base_url);
    creds
}

fn no_retry_http() -> HttpClient {
    let mut config = ClientConfig::default();
    config.retry = RetryPolicy {
        max_retries: 0,
        ..RetryPolicy::default()
    };
    HttpClient::new(config).expect("http")
}

#[tokio::test]
async fn decisions_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .and(body_partial_json(json!({ "model": "openai:gpt-6-luna" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .mount(&server)
        .await;

    let response = decisions(
        &no_retry_http(),
        &openai_creds(&server.uri()),
        action_request(),
    )
    .await
    .expect("decisions");
    let answer = response.answer("action").expect("action");
    assert_eq!(answer.choice_text(), Some("bet"));
    assert_eq!(answer.confidence(), Some(0.7));
    assert_eq!(response.usage.input_tokens, Some(20));
}

#[tokio::test]
async fn decisions_validation_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "error": { "message": "questions[0].instructions is required" }
        })))
        .mount(&server)
        .await;

    let err = decisions(
        &no_retry_http(),
        &openai_creds(&server.uri()),
        action_request(),
    )
    .await
    .err()
    .expect("422");
    assert!(matches!(err, DecisionError::Validation(_)));
}

#[tokio::test]
async fn decisions_rejects_an_empty_question_list() {
    let err = decisions(
        &no_retry_http(),
        &openai_creds("http://127.0.0.1:9"),
        DecisionRequest::new("Hero holds 3.", Vec::new()),
    )
    .await
    .err()
    .expect("empty");
    assert!(matches!(err, DecisionError::Validation(_)));
}

#[tokio::test]
async fn decisions_rate_limit_without_retry() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": { "message": "rate limited" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let err = decisions(
        &no_retry_http(),
        &openai_creds(&server.uri()),
        action_request(),
    )
    .await
    .err()
    .expect("429");
    assert!(matches!(err, DecisionError::Http(_)));
    server.verify().await;
}

#[tokio::test]
async fn decisions_missing_key() {
    let err = decisions(
        &no_retry_http(),
        &ProviderCredentials::new(),
        action_request(),
    )
    .await
    .err()
    .expect("missing key");
    assert!(matches!(err, DecisionError::Credentials(_)));
}

#[tokio::test]
async fn client_decisions_uses_provider_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-unused")
        .api_key_for(ProviderId::OpenAi, "sk-test")
        .base_url_for(ProviderId::OpenAi, server.uri())
        .build()
        .expect("client");
    let response = client
        .decisions(action_request())
        .await
        .expect("client decisions");
    assert_eq!(
        response
            .answer("action")
            .and_then(|answer| answer.choice_text()),
        Some("bet")
    );
}
