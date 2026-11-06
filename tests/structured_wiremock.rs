//! Integration tests for [`Client::complete_object`].

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use superglue::chat::ChatError;
use superglue::client::{CallOptions, Client};
use superglue::openai::JsonSchemaFormat;

fn mock_client(base_url: String) -> Client {
    Client::builder()
        .api_key("sk-test")
        .model("mock-model")
        .base_url(base_url)
        .max_retries(0)
        .build()
        .expect("client")
}

fn person_schema() -> JsonSchemaFormat {
    JsonSchemaFormat {
        name: "person".into(),
        description: None,
        schema: json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"],
            "additionalProperties": false
        }),
        strict: None,
    }
}

fn chat_body(content: &str) -> Value {
    json!({
        "id": "chatcmpl-test",
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
    })
}

type Bodies = Arc<Mutex<Vec<Value>>>;

fn record(bodies: &Bodies, request: &wiremock::Request) {
    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    bodies.lock().expect("bodies").push(body);
}

#[tokio::test]
async fn valid_json_returns_the_value() {
    let server = MockServer::start().await;
    let bodies: Bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |request: &wiremock::Request| {
            record(&seen, request);
            ResponseTemplate::new(200).set_body_json(chat_body("{\"name\":\"ada\"}"))
        })
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let object = client
        .complete_object("name a person", person_schema(), CallOptions::default())
        .await
        .expect("object");

    assert_eq!(object.value, json!({ "name": "ada" }));
    let bodies = bodies.lock().expect("bodies");
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["response_format"]["type"], "json_schema");
    assert_eq!(
        bodies[0]["response_format"]["json_schema"]["name"],
        "person"
    );
    assert_eq!(bodies[0]["response_format"]["json_schema"]["strict"], true);
}

#[tokio::test]
async fn prose_then_valid_json_uses_one_retry() {
    let server = MockServer::start().await;
    let bodies: Bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    let count = Arc::new(AtomicU32::new(0));
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |request: &wiremock::Request| {
            record(&seen, request);
            let n = count.fetch_add(1, Ordering::SeqCst);
            let content = if n == 0 {
                "Here is the person you asked for."
            } else {
                "{\"name\":\"ada\"}"
            };
            ResponseTemplate::new(200).set_body_json(chat_body(content))
        })
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let object = client
        .complete_object("name a person", person_schema(), CallOptions::default())
        .await
        .expect("object after retry");

    assert_eq!(object.value, json!({ "name": "ada" }));
    let bodies = bodies.lock().expect("bodies");
    assert_eq!(bodies.len(), 2);
    let retry_messages = bodies[1]["messages"].as_array().expect("messages");
    let last = retry_messages.last().expect("repair note");
    let note = last["content"].as_str().expect("note text");
    assert!(note.contains("Return only a JSON object"), "note: {note}");
    assert!(
        !note.contains("Here is the person"),
        "note leaks model text"
    );
}

#[tokio::test]
async fn two_bad_replies_return_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("not json")))
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let err = client
        .complete_object("name a person", person_schema(), CallOptions::default())
        .await
        .expect_err("two bad replies");

    assert!(
        matches!(&err, ChatError::Object(kind) if kind == "json"),
        "got: {err:?}"
    );
    assert!(
        !err.to_string().contains("not json"),
        "error leaks model text"
    );
}

#[tokio::test]
async fn a_bad_schema_makes_no_http_call() {
    let server = MockServer::start().await;
    let bodies: Bodies = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&bodies);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |request: &wiremock::Request| {
            record(&seen, request);
            ResponseTemplate::new(200).set_body_json(chat_body("{\"name\":\"ada\"}"))
        })
        .expect(0)
        .mount(&server)
        .await;

    let client = mock_client(server.uri());
    let mut schema = person_schema();
    schema.schema = json!({ "type": "not-a-json-schema-type" });
    let err = client
        .complete_object("name a person", schema, CallOptions::default())
        .await
        .expect_err("bad schema");

    assert!(
        matches!(&err, ChatError::Object(kind) if kind == "schema document"),
        "got: {err:?}"
    );
    assert!(bodies.lock().expect("bodies").is_empty());
}
