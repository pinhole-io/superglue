//! Two agent nodes against Wiremock.

use superglue::Client;
use superglue::agents::AgentSpec;
use superglue::chat::ChatOptions;
use superglue::graph::Graph;
use superglue::guardrails::GuardrailRegistry;
use superglue::hooks::HookRegistry;
use superglue::http::{ClientConfig, HttpClient, RetryPolicy};
use superglue::tools::ToolRegistry;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_body(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-graph",
        "object": "chat.completion",
        "model": "mock",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }]
    })
}

#[tokio::test]
async fn agent_graph_writes_the_second_node() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("Write a draft about"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("DRAFT")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_string_contains("Polish this draft"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("POLISHED")))
        .expect(1)
        .mount(&server)
        .await;

    let mut http_config = ClientConfig::default();
    http_config.retry = RetryPolicy {
        max_retries: 0,
        ..RetryPolicy::default()
    };
    let options = ChatOptions {
        api_key: secrecy::SecretString::from("sk-test".to_string()),
        base_url: server.uri(),
        model: "mock".into(),
        max_tool_rounds: 2,
        ..ChatOptions::default()
    };
    let client = Client::from_parts(
        options,
        std::sync::Arc::new(ToolRegistry::new()),
        std::sync::Arc::new(HookRegistry::new()),
        std::sync::Arc::new(GuardrailRegistry::new()),
        std::sync::Arc::new(HttpClient::new(http_config).unwrap()),
    );
    let spec = AgentSpec::new("writer", "a careful writer");
    let graph = Graph::builder("review")
        .require("question")
        .agent("draft", spec.clone(), "Write a draft about {question}")
        .agent("polish", spec, "Polish this draft:\n{draft}")
        .depends("polish", ["draft"])
        .output("polish")
        .build()
        .unwrap();

    let answer = client
        .run_graph(&graph, serde_json::json!({"question": "ownership"}))
        .await
        .unwrap();
    assert_eq!(answer, serde_json::json!("POLISHED"));
}
