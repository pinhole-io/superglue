//! Judge config runs against Wiremock.

use std::time::Duration;

use serde_json::json;
use superglue::http::{ClientConfig, HttpClient, RetryPolicy};
use superglue::judge::{
    JudgeConfig, JudgeError, JudgeInput, JudgeItem, JudgeOverrides, OnError, run,
};
use superglue::providers::{ProviderCredentials, ProviderId};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn no_retry_http() -> HttpClient {
    let mut config = ClientConfig::default();
    config.retry = RetryPolicy {
        max_retries: 0,
        ..RetryPolicy::default()
    };
    HttpClient::new(config).expect("http")
}

fn typesafe_creds(base_url: &str) -> ProviderCredentials {
    let mut creds = ProviderCredentials::new();
    creds.insert_key(ProviderId::TypeSafe, "ts-test");
    creds.insert_base_url(ProviderId::TypeSafe, base_url);
    creds
}

fn choice_body(choice: &str, confidence: f64) -> serde_json::Value {
    json!({
        "model": "jev-latest",
        "answers": {
            "gate": {
                "type": "choice",
                "choice": choice,
                "probabilities": {},
                "confidence": confidence
            }
        },
        "usage": { "input_tokens": 4, "output_tokens": 1 }
    })
}

fn gate_config(on_error: &str) -> JudgeConfig {
    JudgeConfig::from_json(&format!(
        r#"{{
            "id": "gate",
            "on_error": "{on_error}",
            "questions": {{
                "gate": {{
                    "type": "choice",
                    "instructions": "Which gate?",
                    "options": {{ "brief": "Plan first.", "direct": "Answer now." }}
                }}
            }},
            "outputs": [{{
                "kind": "choice",
                "name": "expand",
                "from": "gate",
                "min_confidence": 0.6,
                "map": {{ "brief": true, "direct": false }},
                "required": true
            }}]
        }}"#
    ))
    .expect("config")
}

#[tokio::test]
async fn mapped_bool_comes_from_choice() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(choice_body("brief", 0.9)))
        .mount(&server)
        .await;

    let verdict = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &gate_config("error"),
        JudgeInput::text("plan the crawl"),
        JudgeOverrides::default(),
    )
    .await
    .expect("judge");
    assert_eq!(verdict.bool("expand"), Some(true));
}

#[tokio::test]
async fn low_confidence_omits_the_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(choice_body("brief", 0.2)))
        .mount(&server)
        .await;

    let mut config = gate_config("error");
    if let superglue::judge::OutputSpec::Choice { required, .. } = &mut config.outputs[0] {
        *required = false;
    }
    let verdict = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &config,
        JudgeInput::text("hello"),
        JudgeOverrides::default(),
    )
    .await
    .expect("judge");
    assert!(verdict.bool("expand").is_none());
    assert!(verdict.is_empty());
}

#[tokio::test]
async fn repeat_map_keeps_answered_items() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-latest",
            "answers": {
                "mem:a": { "type": "score", "score": 1.5, "legend": {}, "confidence": 0.8 }
            },
            "usage": { "input_tokens": 4, "output_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let config = JudgeConfig::from_json(
        r#"{
            "id": "rerank",
            "on_error": "empty",
            "repeat": {
                "id_prefix": "mem:",
                "template": {
                    "type": "score",
                    "instructions": "How useful is this memory?",
                    "levels": ["Low", "Mid", "High"]
                },
                "output": { "kind": "score", "name": "scores" }
            }
        }"#,
    )
    .expect("config");
    let verdict = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &config,
        JudgeInput::json(json!({ "query": "ship it" }))
            .with_items(vec![JudgeItem::new("a"), JudgeItem::new("b")]),
        JudgeOverrides::default(),
    )
    .await
    .expect("judge");
    let scores = verdict.map("scores").expect("scores");
    assert_eq!(
        scores.get("a").and_then(|value| value.as_number()),
        Some(1.5)
    );
    assert!(scores.get("b").is_none());
}

#[tokio::test]
async fn upstream_error_returns_the_error_policy() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": { "message": "upstream exploded" }
        })))
        .mount(&server)
        .await;

    let err = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &gate_config("error"),
        JudgeInput::text("hello"),
        JudgeOverrides::default(),
    )
    .await
    .expect_err("error policy");
    assert!(matches!(err, JudgeError::Upstream));
    assert!(!err.to_string().contains("exploded"));

    let verdict = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &gate_config("empty"),
        JudgeInput::text("hello"),
        JudgeOverrides::default(),
    )
    .await
    .expect("empty policy");
    assert!(verdict.is_empty());
    assert!(matches!(gate_config("empty").on_error, OnError::Empty));
}

#[tokio::test]
async fn timeout_follows_the_error_policy() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(choice_body("direct", 0.9))
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&server)
        .await;

    let err = run(
        &no_retry_http(),
        &typesafe_creds(&server.uri()),
        &gate_config("error"),
        JudgeInput::text("hello"),
        JudgeOverrides {
            timeout: Some(Duration::from_millis(40)),
            ..JudgeOverrides::default()
        },
    )
    .await
    .expect_err("timeout");
    assert!(matches!(err, JudgeError::Timeout));
}
