use serde_json::json;
use superglue::Client;
use superglue::embeddings::{EmbedError, EmbeddingInput, EmbeddingRequest, embed};
use superglue::http::{ClientConfig, HttpClient};
use superglue::providers::{ProviderCredentials, ProviderId};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn client_returns_embedding_and_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .and(body_json(json!({
            "model": "text-embedding-3-small",
            "input": "hello",
            "dimensions": 3
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{
                "object": "embedding",
                "index": 0,
                "embedding": [0.1, 0.2, 0.3]
            }],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 1, "total_tokens": 1}
        })))
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk-test")
        .base_url(server.uri())
        .build()
        .expect("client");
    let outcome = client
        .embed("text-embedding-3-small", "hello", Some(3))
        .await
        .expect("embedding");

    assert_eq!(outcome.model, "text-embedding-3-small");
    assert_eq!(outcome.embeddings.len(), 1);
    assert_eq!(outcome.embeddings[0].vector, vec![0.1, 0.2, 0.3]);
    assert_eq!(outcome.usage.expect("usage").total_tokens, 1);
}

#[tokio::test]
async fn batch_input_returns_indexed_vectors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/embeddings"))
        .and(body_json(json!({
            "model": "text-embedding-3-small",
            "input": ["first", "second"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"index": 0, "embedding": [0.1, 0.2]},
                {"index": 1, "embedding": [0.3, 0.4]}
            ],
            "model": "text-embedding-3-small"
        })))
        .mount(&server)
        .await;

    let (http, credentials) = test_client(&server);
    let outcome = embed(
        &http,
        &credentials,
        EmbeddingRequest {
            model: "text-embedding-3-small".into(),
            input: EmbeddingInput::Texts(vec!["first".into(), "second".into()]),
            dimensions: None,
        },
    )
    .await
    .expect("batch embeddings");

    assert_eq!(outcome.embeddings.len(), 2);
    assert_eq!(outcome.embeddings[0].index, 0);
    assert_eq!(outcome.embeddings[1].vector, vec![0.3, 0.4]);
}

#[tokio::test]
async fn anthropic_is_unsupported() {
    let http = HttpClient::new(ClientConfig::default()).expect("http client");
    let mut credentials = ProviderCredentials::new();
    credentials.insert_key(ProviderId::Anthropic, "sk-ant");

    let error = embed(
        &http,
        &credentials,
        EmbeddingRequest {
            model: "anthropic:claude".into(),
            input: EmbeddingInput::Text("hello".into()),
            dimensions: None,
        },
    )
    .await
    .expect_err("Anthropic has no embeddings endpoint");

    assert!(matches!(
        error,
        EmbedError::UnsupportedProvider(ProviderId::Anthropic)
    ));
}

#[tokio::test]
async fn empty_input_is_rejected_before_http() {
    let http = HttpClient::new(ClientConfig::default()).expect("http client");
    let credentials = ProviderCredentials::new();

    let error = embed(
        &http,
        &credentials,
        EmbeddingRequest {
            model: "text-embedding-3-small".into(),
            input: EmbeddingInput::Text(" \n ".into()),
            dimensions: None,
        },
    )
    .await
    .expect_err("empty text must fail");

    assert!(matches!(error, EmbedError::EmptyInput));
}

fn test_client(server: &MockServer) -> (HttpClient, ProviderCredentials) {
    let http = HttpClient::new(ClientConfig::default()).expect("http client");
    let mut credentials = ProviderCredentials::new();
    credentials.insert_key(ProviderId::OpenAi, "sk-test");
    credentials.insert_base_url(ProviderId::OpenAi, server.uri());
    (http, credentials)
}
