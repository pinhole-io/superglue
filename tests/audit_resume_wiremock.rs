//! Wiremock tests for audit resume and process event recording.

use std::sync::Arc;

use serde_json::json;
use superglue::audit::{
    AuditStore, FinishMetadata, JsonlRunStore, MemoryRunStore, RunRecorder, resume_chat,
    resume_context_from_record, resume_response,
};
use superglue::chat::ChatOptions;
use superglue::client::{CallOptions, Client};
use superglue::events::{ProcessEvent, ProcessEventKind, StatusEmitter, StatusSubscriber};
use superglue::guardrails::GuardrailRegistry;
use superglue::hooks::HookRegistry;
use superglue::http::{ClientConfig, HttpClient};
use superglue::proto;
use superglue::tools::ToolRegistry;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn runrecorder_process_events_via_status_subscriber() {
    let store = Arc::new(MemoryRunStore::new(None));
    let recorder = Arc::new(RunRecorder::new(store.clone()));
    let emitter = Arc::new(StatusEmitter::new());
    emitter
        .subscribe(Arc::clone(&recorder) as Arc<dyn StatusSubscriber>)
        .await;

    emitter
        .emit(ProcessEvent::new(
            ProcessEventKind::LlmCallStart,
            "req-pe",
            "gpt-4o-mini",
        ))
        .await;
    emitter
        .emit(ProcessEvent::new(
            ProcessEventKind::LlmCallEnd,
            "req-pe",
            "gpt-4o-mini",
        ))
        .await;

    recorder
        .finish_with_metadata(
            "req-pe".into(),
            vec![],
            proto::CompletionOutcome {
                content: Some("ok".into()),
                rounds: 1,
                usage: None,
                finish_reason: Some("stop".into()),
            },
            100,
            200,
            FinishMetadata {
                options_json: Some(r#"{"model":"gpt-4o-mini"}"#.into()),
                model_used: Some("gpt-4o-mini".into()),
                previous_response_id: None,
            },
        )
        .await;

    let record = store.get("req-pe").await.expect("record stored");
    assert_eq!(record.process_events.len(), 2);
    assert_eq!(
        record.process_events[0].kind,
        proto::ProcessEventKind::LlmCallStart as i32
    );
    assert_eq!(
        record.process_events[1].kind,
        proto::ProcessEventKind::LlmCallEnd as i32
    );
}

#[tokio::test]
async fn resume_chat_mid_tool_round() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            let messages = body
                .get("messages")
                .and_then(|m| m.as_array())
                .expect("messages");
            assert!(
                messages
                    .iter()
                    .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("tool")),
                "resume must include tool history: {messages:?}"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-resume",
                "model": "mock",
                "choices": [{
                    "message": { "role": "assistant", "content": "finished" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7 }
            }))
        })
        .mount(&server)
        .await;

    let options_json = superglue::audit::options_snapshot(&ChatOptions {
        base_url: server.uri(),
        api_key: secrecy::SecretString::from("sk-test".to_string()),
        model: "mock".into(),
        max_tool_rounds: 4,
        ..Default::default()
    });

    let record = proto::RunRecord {
        request_id: "req-chat-resume".into(),
        messages: vec![
            proto::ChatMessage {
                role: "user".into(),
                content: Some("call echo".into()),
                tool_calls: vec![],
                tool_call_id: None,
                name: None,
                refusal: None,
                provider_blocks_json: None,
            },
            proto::ChatMessage {
                role: "assistant".into(),
                content: None,
                tool_calls: vec![proto::ToolCall {
                    id: "call_1".into(),
                    kind: "function".into(),
                    function: Some(proto::FunctionCall {
                        name: "echo".into(),
                        arguments: r#"{"x":1}"#.into(),
                    }),
                }],
                tool_call_id: None,
                name: None,
                refusal: None,
                provider_blocks_json: None,
            },
            proto::ChatMessage {
                role: "tool".into(),
                content: Some(r#"{"echo":{"x":1}}"#.into()),
                tool_calls: vec![],
                tool_call_id: Some("call_1".into()),
                name: Some("echo".into()),
                refusal: None,
                provider_blocks_json: None,
            },
        ],
        hook_events: vec![],
        outcome: None,
        started_at_ms: 0,
        finished_at_ms: 1,
        process_events: vec![],
        options_json: Some(options_json),
        model_used: Some("mock".into()),
        previous_response_id: None,
    };

    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let out = resume_chat(
        &http,
        &ToolRegistry::new(),
        &HookRegistry::new(),
        &GuardrailRegistry::new(),
        &record,
        ChatOptions {
            base_url: server.uri(),
            api_key: secrecy::SecretString::from("sk-test".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(out.content.as_deref(), Some("finished"));
}

#[test]
fn resume_response_opts_carry_previous_response_id() {
    let options_json = superglue::audit::options_snapshot(&ChatOptions {
        model: "mock".into(),
        ..Default::default()
    });
    let record = proto::RunRecord {
        request_id: "req".into(),
        messages: vec![proto::ChatMessage {
            role: "user".into(),
            content: Some("continue".into()),
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks_json: None,
        }],
        hook_events: vec![],
        outcome: None,
        started_at_ms: 0,
        finished_at_ms: 1,
        process_events: vec![],
        options_json: Some(options_json),
        model_used: Some("mock".into()),
        previous_response_id: Some("resp_tool".into()),
    };
    let ctx = resume_context_from_record(&record, ChatOptions::default()).unwrap();
    assert_eq!(ctx.previous_response_id.as_deref(), Some("resp_tool"));
    let mut opts = ctx.options;
    if let Some(id) = ctx.previous_response_id {
        let mut extra = opts.extra_json.clone().unwrap_or(json!({}));
        if let serde_json::Value::Object(ref mut map) = extra {
            map.insert("previous_response_id".into(), json!(id));
        }
        opts.extra_json = Some(extra);
    }
    assert_eq!(
        opts.extra_json
            .as_ref()
            .and_then(|e| e.get("previous_response_id"))
            .and_then(|v| v.as_str()),
        Some("resp_tool")
    );
}

#[tokio::test]
async fn resume_response_threads_previous_response_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or(json!({}));
            assert_eq!(
                body.get("previous_response_id").and_then(|v| v.as_str()),
                Some("resp_tool")
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "resp_done",
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "resumed" }]
                }],
                "usage": { "input_tokens": 8, "output_tokens": 3, "total_tokens": 11 }
            }))
        })
        .mount(&server)
        .await;

    let options_json = superglue::audit::options_snapshot(&ChatOptions {
        base_url: server.uri(),
        api_key: secrecy::SecretString::from("sk-test".to_string()),
        model: "mock".into(),
        max_tool_rounds: 4,
        ..Default::default()
    });

    let record = proto::RunRecord {
        request_id: "req-resp-resume".into(),
        messages: vec![proto::ChatMessage {
            role: "user".into(),
            content: Some("continue".into()),
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
            refusal: None,
            provider_blocks_json: None,
        }],
        hook_events: vec![],
        outcome: None,
        started_at_ms: 0,
        finished_at_ms: 1,
        process_events: vec![],
        options_json: Some(options_json),
        model_used: Some("mock".into()),
        previous_response_id: Some("resp_tool".into()),
    };

    let http = HttpClient::new(ClientConfig::default()).unwrap();
    let out = resume_response(
        &http,
        &ToolRegistry::new(),
        &HookRegistry::new(),
        &GuardrailRegistry::new(),
        &record,
        ChatOptions {
            base_url: server.uri(),
            api_key: secrecy::SecretString::from("sk-test".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(out.content.as_deref(), Some("resumed"));
}

#[tokio::test]
async fn client_builder_audit_records_chat_completion() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-audit",
            "model": "mock",
            "choices": [{
                "message": { "role": "assistant", "content": "recorded" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5 }
        })))
        .mount(&server)
        .await;

    let store = Arc::new(MemoryRunStore::new(None));
    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .model("mock")
        .audit(store.clone())
        .build()
        .unwrap();

    let out = client
        .complete("hello audit", CallOptions::default())
        .await
        .unwrap();

    let record = store
        .get(&out.request_id)
        .await
        .expect("audit record stored");
    assert_eq!(
        record.outcome.as_ref().and_then(|o| o.content.as_deref()),
        Some("recorded")
    );
    assert!(
        record
            .options_json
            .as_ref()
            .is_some_and(|s| s.contains("mock"))
    );
    assert_eq!(record.model_used.as_deref(), Some("mock"));
    assert!(
        record
            .hook_events
            .iter()
            .any(|e| e.stage == "pre_completion")
    );
}

#[tokio::test]
async fn client_jsonl_audit_writes_steps_without_secrets() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-jsonl",
            "model": "mock",
            "choices": [{
                "message": { "role": "assistant", "content": "on disk" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5 }
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(JsonlRunStore::open(dir.path()).unwrap());
    let api_key = "sk-jsonl-secret-value";
    let client = Client::builder()
        .api_key(api_key)
        .base_url(server.uri())
        .model("mock")
        .audit(store.clone())
        .build()
        .unwrap();

    let out = client
        .complete("hello disk", CallOptions::default())
        .await
        .unwrap();

    let file = dir.path().join(format!("{}.jsonl", out.request_id));
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        !text.contains(api_key),
        "audit log must not hold the API key"
    );
    let kinds: Vec<String> = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["kind"].to_string())
        .collect();
    assert_eq!(
        kinds,
        [
            "\"run_started\"",
            "\"message_appended\"",
            "\"message_appended\"",
            "\"run_finished\""
        ]
    );

    let record = store.get(&out.request_id).await.expect("record replayed");
    let contents: Vec<_> = record
        .messages
        .iter()
        .map(|m| (m.role.as_str(), m.content.as_deref()))
        .collect();
    assert_eq!(
        contents,
        [("user", Some("hello disk")), ("assistant", Some("on disk"))]
    );
    assert_eq!(
        record.outcome.and_then(|o| o.content).as_deref(),
        Some("on disk")
    );
    let options_json = record.options_json.expect("options on run_started");
    assert!(options_json.contains("mock"));
    assert!(!options_json.contains(api_key));
}
