#![cfg(feature = "gateway")]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use superglue::gateway::{router, test_state, test_state_with_openai};

const MASTER_KEY: &str = "test-master-key-12345";
const PROXY_KEY: &str = "sk-proxy-secret-zzz9";

fn auth_request(
    method: &str,
    uri: &str,
    key: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Superglue-Key", format!("Bearer {key}"));
    if let Some(body) = body {
        builder = builder.header("content-type", "application/json");
        builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

async fn body_json(body: Body) -> serde_json::Value {
    let bytes = body.collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!({ "raw": String::from_utf8_lossy(&bytes) }))
}

async fn create_user(app: &axum::Router, user_id: &str) {
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/v1/users",
            MASTER_KEY,
            Some(json!({ "user_id": user_id })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

async fn create_virtual_key(app: &axum::Router, user_id: &str, models: &[&str]) -> String {
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/v1/keys",
            MASTER_KEY,
            Some(json!({
                "user_id": user_id,
                "allowed_models": models,
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response.into_body()).await["key"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn create_proxy(
    app: &axum::Router,
    slug: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> serde_json::Value {
    let mut body = json!({
        "slug": slug,
        "base_url": base_url,
        "enabled": true,
    });
    if let Some(api_key) = api_key {
        body["api_key"] = json!(api_key);
    }
    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/v1/proxy-providers",
            MASTER_KEY,
            Some(body),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "create proxy failed");
    body_json(response.into_body()).await
}

fn chat_completion(model: &str) -> serde_json::Value {
    json!({
        "id": "cmpl-proxy",
        "object": "chat.completion",
        "created": 1,
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "ok" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 4,
            "completion_tokens": 2,
            "total_tokens": 6,
            "cost": 1.5
        }
    })
}

#[tokio::test]
async fn proxy_chat_forwards_the_bare_model_and_stored_bearer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("org/model:tag")))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let state = test_state_with_openai(
        MASTER_KEY,
        &dir.path().join("gw.db"),
        &server.uri(),
        "sk-builtin",
    );
    let app = router(state.clone());
    create_user(&app, "alice").await;
    let created = create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;
    assert_eq!(created["key_suffix"], "...zzz9");
    assert!(created.get("api_key").is_none());
    let key = create_virtual_key(&app, "alice", &["homelab:*"]).await;

    let response = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            &key,
            Some(json!({
                "model": "homelab:org/model:tag",
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let authorization = requests[0]
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(authorization, format!("Bearer {PROXY_KEY}"));
    assert_ne!(authorization, format!("Bearer {key}"));
    let sent: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(sent["model"], "org/model:tag");

    let usage = state.db.list_usage(None, None, 10).unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].model, "homelab:org/model:tag");
    assert_eq!(usage[0].cost_usd, 1.5);
}

#[tokio::test]
async fn proxy_chat_stream_records_usage() {
    let server = MockServer::start().await;
    let sse = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let state = test_state(MASTER_KEY, &dir.path().join("gw.db"));
    let app = router(state.clone());
    create_user(&app, "alice").await;
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;
    let key = create_virtual_key(&app, "alice", &["homelab:*"]).await;

    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            &key,
            Some(json!({
                "model": "homelab:llama3.1",
                "stream": true,
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let _ = response.into_body().collect().await.unwrap();

    let usage = state.db.list_usage(None, None, 10).unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].prompt_tokens, 3);
    assert_eq!(usage[0].completion_tokens, 1);
    assert_eq!(usage[0].cost_usd, 0.0);
}

#[tokio::test]
async fn proxy_responses_and_embeddings_forward_the_bare_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1",
            "model": "llama3.1",
            "output": [],
            "usage": { "input_tokens": 2, "output_tokens": 1, "total_tokens": 3 }
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.1] }],
            "model": "bge-m3",
            "usage": { "prompt_tokens": 2, "total_tokens": 2 }
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let state = test_state(MASTER_KEY, &dir.path().join("gw.db"));
    let app = router(state);
    create_user(&app, "alice").await;
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;
    let key = create_virtual_key(&app, "alice", &["homelab:*"]).await;

    let responses = app
        .clone()
        .oneshot(auth_request(
            "POST",
            "/v1/responses",
            &key,
            Some(json!({
                "model": "homelab:llama3.1",
                "input": "hello"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(responses.status(), StatusCode::OK);

    let embeddings = app
        .oneshot(auth_request(
            "POST",
            "/v1/embeddings",
            &key,
            Some(json!({
                "model": "homelab:embedding:bge-m3",
                "input": "hello"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(embeddings.status(), StatusCode::OK);

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(
            request
                .headers
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            format!("Bearer {PROXY_KEY}")
        );
        let sent: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        let model = sent["model"].as_str().unwrap();
        assert!(model == "llama3.1" || model == "bge-m3", "{model}");
    }
}

#[tokio::test]
async fn missing_proxy_slug_is_400_and_does_not_call_openai() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("llama")))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let state = test_state_with_openai(
        MASTER_KEY,
        &dir.path().join("gw.db"),
        &server.uri(),
        "sk-builtin",
    );
    let app = router(state);
    create_user(&app, "alice").await;

    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            MASTER_KEY,
            Some(json!({
                "user": "alice",
                "model": "nope:llama",
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = body_json(response.into_body()).await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown provider")
    );
}

#[tokio::test]
async fn builtin_slug_is_rejected_at_create() {
    let dir = tempfile::tempdir().unwrap();
    let state = test_state(MASTER_KEY, &dir.path().join("gw.db"));
    let app = router(state);
    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/proxy-providers",
            MASTER_KEY,
            Some(json!({
                "slug": "openai",
                "base_url": "http://127.0.0.1:9"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn proxy_without_api_key_omits_authorization() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("llama3.1")))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let app = router(test_state(MASTER_KEY, &dir.path().join("gw.db")));
    create_user(&app, "alice").await;
    create_proxy(&app, "homelab", &server.uri(), None).await;
    let key = create_virtual_key(&app, "alice", &["homelab:llama3.1"]).await;

    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            &key,
            Some(json!({
                "model": "homelab:llama3.1",
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let requests = server.received_requests().await.unwrap();
    assert!(requests[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn proxy_redirect_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/stolen", server.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/stolen"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("llama")))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let app = router(test_state(MASTER_KEY, &dir.path().join("gw.db")));
    create_user(&app, "alice").await;
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;
    let key = create_virtual_key(&app, "alice", &["homelab:*"]).await;

    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            &key,
            Some(json!({
                "model": "homelab:llama3.1",
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::OK);
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|request| request.url.path() != "/stolen")
    );
}

#[tokio::test]
async fn proxy_model_list_prefixes_upstream_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "llama3.1", "object": "model" }]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let app = router(test_state_with_openai(
        MASTER_KEY,
        &dir.path().join("gw.db"),
        &server.uri(),
        "sk-builtin",
    ));
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;

    let response = app
        .oneshot(auth_request("GET", "/v1/models", MASTER_KEY, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response.into_body()).await;
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert!(ids.contains(&"homelab:llama3.1"), "{ids:?}");
}

#[tokio::test]
async fn disabled_proxy_returns_403() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion("llama3.1")))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let app = router(test_state(MASTER_KEY, &dir.path().join("gw.db")));
    create_user(&app, "alice").await;
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;
    let updated = app
        .clone()
        .oneshot(auth_request(
            "PATCH",
            "/v1/proxy-providers/homelab",
            MASTER_KEY,
            Some(json!({ "enabled": false })),
        ))
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    let key = create_virtual_key(&app, "alice", &["homelab:*"]).await;

    let response = app
        .oneshot(auth_request(
            "POST",
            "/v1/chat/completions",
            &key,
            Some(json!({
                "model": "homelab:llama3.1",
                "messages": [{ "role": "user", "content": "hi" }]
            })),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn proxy_admin_list_returns_a_suffix_and_not_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "llama3.1", "object": "model" }]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let app = router(test_state_with_openai(
        MASTER_KEY,
        &dir.path().join("gw.db"),
        &server.uri(),
        "sk-builtin",
    ));
    create_proxy(&app, "homelab", &server.uri(), Some(PROXY_KEY)).await;

    let response = app
        .clone()
        .oneshot(auth_request("GET", "/v1/proxy-providers", MASTER_KEY, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains(PROXY_KEY));
    assert!(text.contains("...zzz9"));

    let providers = app
        .oneshot(auth_request("GET", "/v1/providers", MASTER_KEY, None))
        .await
        .unwrap();
    let listed = body_json(providers.into_body()).await;
    let proxy = listed["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "homelab")
        .unwrap();
    assert_eq!(proxy["kind"], "proxy");
    assert_eq!(proxy["key_suffix"], "...zzz9");
    assert!(!listed.to_string().contains(PROXY_KEY));
}
