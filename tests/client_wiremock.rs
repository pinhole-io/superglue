//! Integration tests for [`superglue::Client`].

use std::sync::Arc;

use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use superglue::client::{CallOptions, Client};
use superglue::tools::{Tool, ToolInvokeError, ToolSpec};

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".to_string(),
            description: Some("echo args".into()),
            parameters_schema: json!({"type": "object"}),
            static_tool: false,
        }
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvokeError> {
        Ok(json!({ "echo": arguments }))
    }
}

fn mock_client(base_url: String) -> Client {
    Client::builder()
        .api_key("sk-test")
        .model("mock-model")
        .base_url(base_url)
        .max_retries(0)
        .build()
        .expect("client")
}

#[tokio::test]
async fn client_complete_smoke() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "model": "mock-model",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "hello" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let outcome = client
        .complete("hi", CallOptions::default())
        .await
        .expect("complete");

    assert_eq!(outcome.content.as_deref(), Some("hello"));
    assert_eq!(outcome.rounds, 1);
}

#[tokio::test]
async fn client_tool_round() {
    let server = MockServer::start().await;
    let n = Arc::new(AtomicU32::new(0));
    let n2 = Arc::clone(&n);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_: &wiremock::Request| {
            let count = n2.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "chatcmpl-tool",
                    "model": "mock-model",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {
                                    "name": "echo",
                                    "arguments": "{\"message\":\"hi\"}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "chatcmpl-final",
                    "model": "mock-model",
                    "choices": [{
                        "index": 0,
                        "message": { "role": "assistant", "content": "done" },
                        "finish_reason": "stop"
                    }]
                }))
            }
        })
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    client
        .register_tool(Arc::new(EchoTool) as Arc<dyn Tool>)
        .await
        .expect("register");

    let outcome = client
        .complete("use echo", CallOptions::default())
        .await
        .expect("complete with tool");

    assert_eq!(outcome.content.as_deref(), Some("done"));
    assert!(outcome.rounds >= 2);
}

#[tokio::test]
async fn jsonl_audit_keeps_tool_round_when_next_call_fails() {
    use superglue::audit::{AuditStore, JsonlRunStore};

    let server = MockServer::start().await;
    let n = Arc::new(AtomicU32::new(0));
    let n2 = Arc::clone(&n);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_: &wiremock::Request| {
            if n2.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_json(json!({
                    "id": "chatcmpl-tool",
                    "model": "mock-model",
                    "choices": [{
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": { "name": "echo", "arguments": "{}" }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                }))
            } else {
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": { "message": "provider rejected the call" }
                }))
            }
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(JsonlRunStore::open(dir.path()).unwrap());
    let client = Client::builder()
        .api_key("sk-test")
        .model("mock-model")
        .base_url(server.uri())
        .max_retries(0)
        .audit(store.clone())
        .build()
        .expect("client");
    client
        .register_tool(Arc::new(EchoTool) as Arc<dyn Tool>)
        .await
        .expect("register");

    let call = CallOptions {
        request_id: Some("failed-run".into()),
        ..CallOptions::default()
    };
    client
        .complete("use echo", call)
        .await
        .expect_err("second model call fails");

    let text = std::fs::read_to_string(dir.path().join("failed-run.jsonl")).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "run_failed");

    let record = store.get("failed-run").await.expect("partial run");
    let roles: Vec<&str> = record.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["user", "assistant", "tool"]);
    assert!(record.outcome.is_none());
    assert!(record.options_json.is_some());
}
