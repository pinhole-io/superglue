//! Chat completions: non-streaming tool-loop and streaming (SSE) variants.

mod complete;
mod context_ops;
mod conversation;
mod dispatch;
mod error;
mod file_locks;
mod loop_guard;
mod options;
mod provider;
pub mod reasoning;
mod stream;
pub(crate) mod stream_tools;
mod tool_loop;
pub(crate) mod transition;

pub(crate) use loop_guard::{SharedToolLoopGuard, new_tool_loop_guard};

pub(crate) use context_ops::{
    condense_tool_round, estimate_context_chars, maybe_summarize_messages,
};

pub use complete::*;
pub use conversation::Conversation;
pub use error::*;
pub use options::*;
#[cfg(feature = "gateway")]
pub use provider::{proxy_chat_post, proxy_chat_stream};
pub use stream::*;
pub use tool_loop::*;
pub use transition::{TransitionKind, TransitionLog, TransitionNote};

pub(crate) use dispatch::{DispatchCtx, chat_error_class, dispatch_one};
pub(crate) use error::{resolve_chat_provider, should_retry_stalled_stream_round};
#[cfg(test)]
pub(crate) use options::has_system_prompt;
#[cfg(feature = "code")]
pub(crate) use options::refresh_available_tools;
pub(crate) use options::{
    effective_system_prompt, injected_system_prefix_count, log_prefix_guard,
    messages_with_volatile_suffix, notify_llm_payload, prepend_system_messages,
};
pub(crate) use provider::{observation_hook_ctx, post_json_cancellable, provider_chat_post};

#[cfg(test)]
mod truncate_tests {
    use crate::openai::ChatMessage;

    #[test]
    fn truncate_tool_result_appends_marker() {
        let content = "x".repeat(100);
        let out = super::dispatch::truncate_tool_result(content.clone(), 50);
        assert!(out.contains("…\n[truncated]"));
        assert!(out.len() < content.len());
    }

    #[test]
    fn truncate_tool_result_zero_disables() {
        let content = "x".repeat(10_000);
        let out = super::dispatch::truncate_tool_result(content.clone(), 0);
        assert_eq!(out, content);
    }

    #[test]
    fn default_tool_result_cap_is_off() {
        assert_eq!(super::ChatOptions::default().tool_result_max_chars, 0);
    }

    #[test]
    fn conversation_messages_for_client_strips_all_injected_system_rows() {
        let messages = vec![
            ChatMessage::text("system", "stable base"),
            ChatMessage::text("system", "dynamic tail"),
            ChatMessage::text("user", "hello"),
            ChatMessage::text("assistant", "hi"),
        ];
        let client = super::conversation_messages_for_client(&messages, 2, 0);
        assert_eq!(client.len(), 2);
        assert_eq!(client[0].role, "user");
        assert_eq!(client[1].role, "assistant");
    }

    #[test]
    fn conversation_messages_for_client_keeps_caller_system_row() {
        let messages = vec![
            ChatMessage::text("system", "injected"),
            ChatMessage::text("system", "caller supplied"),
            ChatMessage::text("user", "hello"),
        ];
        let client = super::conversation_messages_for_client(&messages, 1, 0);
        assert_eq!(client.len(), 2);
        assert_eq!(client[0].role, "system");
        assert_eq!(
            client[0].content.as_ref().and_then(|c| c.as_text()),
            Some("caller supplied")
        );
    }

    #[test]
    fn conversation_messages_for_client_strips_volatile_suffix() {
        let messages = vec![
            ChatMessage::text("system", "base"),
            ChatMessage::text("user", "hello"),
            ChatMessage::text("system", "todos"),
        ];
        let client = super::conversation_messages_for_client(&messages, 1, 1);
        assert_eq!(client.len(), 1);
        assert_eq!(client[0].role, "user");
    }

    #[tokio::test]
    async fn output_block_confident_no_skips_regen() {
        use crate::http::{ClientConfig, HttpClient, RetryPolicy};
        use crate::providers::{ProviderCredentials, ProviderId};
        use serde_json::json;
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-latest",
                "answers": { "regen": { "type": "noul", "noul": 0.1 } },
                "usage": { "input_tokens": 4, "output_tokens": 1 }
            })))
            .mount(&server)
            .await;

        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::TypeSafe, "ts-test");
        creds.insert_base_url(ProviderId::TypeSafe, server.uri());
        let http = HttpClient::new(ClientConfig {
            retry: RetryPolicy {
                max_retries: 0,
                ..RetryPolicy::default()
            },
            ..ClientConfig::default()
        })
        .expect("http");
        let mut options = super::ChatOptions::default();
        options.provider_credentials = Some(Arc::new(creds));
        assert!(!super::output_block_should_regen(&http, &options, "hello world", "policy").await);
    }

    #[tokio::test]
    async fn output_block_timeout_regens() {
        use crate::http::{ClientConfig, HttpClient, RetryPolicy};
        use crate::providers::{ProviderCredentials, ProviderId};
        use serde_json::json;
        use std::sync::Arc;
        use std::time::Duration;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({
                        "model": "jev-latest",
                        "answers": { "regen": { "type": "noul", "noul": 0.1 } },
                        "usage": { "input_tokens": 4, "output_tokens": 1 }
                    }))
                    .set_delay(Duration::from_secs(2)),
            )
            .mount(&server)
            .await;

        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::TypeSafe, "ts-test");
        creds.insert_base_url(ProviderId::TypeSafe, server.uri());
        let http = HttpClient::new(ClientConfig {
            retry: RetryPolicy {
                max_retries: 0,
                ..RetryPolicy::default()
            },
            ..ClientConfig::default()
        })
        .expect("http");
        let mut options = super::ChatOptions::default();
        options.provider_credentials = Some(Arc::new(creds));
        assert!(super::output_block_should_regen(&http, &options, "hello world", "policy").await);
    }

    #[test]
    fn prepend_system_messages_counts_non_empty_blocks() {
        let mut options = super::ChatOptions::new("https://api.openai.com", "k", "m");
        options.system_prompt_blocks = Some(vec![
            super::SystemPromptBlock::cached("base"),
            super::SystemPromptBlock::uncached("dynamic"),
            super::SystemPromptBlock::uncached(""),
        ]);
        let mut messages = Vec::new();
        let injected = super::prepend_system_messages(&options, &mut messages);
        assert_eq!(injected, 2);
        assert_eq!(messages.len(), 2);
        assert!(super::has_system_prompt(&options));
    }

    #[test]
    fn volatile_suffix_is_sent_and_stripped_from_client_messages() {
        let mut options = super::ChatOptions::new("https://api.openai.com", "k", "m");
        options.system_prompt_blocks = Some(vec![super::SystemPromptBlock::cached("base")]);
        options.volatile_suffix_blocks =
            Some(vec![super::SystemPromptBlock::uncached("session todos")]);
        let mut working = Vec::new();
        let prefix = super::prepend_system_messages(&options, &mut working);
        working.push(ChatMessage::text("user", "hello"));
        let request = super::messages_with_volatile_suffix(&options, &working);
        assert_eq!(request.len(), 3);
        assert_eq!(request[0].role, "system");
        assert_eq!(request[1].role, "user");
        assert_eq!(request[2].role, "system");
        assert_eq!(
            request[2].content.as_ref().and_then(|c| c.as_text()),
            Some("session todos")
        );
        let suffix = super::injected_system_suffix_count(&options);
        let client = super::conversation_messages_for_client(&request, prefix, suffix);
        assert_eq!(client.len(), 1);
        assert_eq!(client[0].role, "user");
    }

    #[tokio::test]
    async fn stream_complete_sends_volatile_suffix_blocks() {
        use crate::http::{ClientConfig, HttpClient};
        use crate::providers::{ProviderCredentials, ProviderId};
        use std::sync::Arc;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":0,",
            "\"model\":\"gpt-4o-mini\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",",
            "\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
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

        let http = HttpClient::new(ClientConfig::default()).expect("http");
        let hooks = crate::hooks::HookRegistry::new();
        let guardrails = crate::guardrails::GuardrailRegistry::new();
        let mut options = super::ChatOptions::new(server.uri(), "test-key", "openai:gpt-4o-mini");
        options.system_prompt_blocks = Some(vec![super::SystemPromptBlock::cached("base")]);
        options.volatile_suffix_blocks = Some(vec![super::SystemPromptBlock::uncached(
            "role: check fresno weather",
        )]);
        let mut creds = ProviderCredentials::new();
        creds.insert_key(ProviderId::OpenAi, "test-key");
        creds.insert_base_url(ProviderId::OpenAi, server.uri());
        options.provider_credentials = Some(Arc::new(creds));

        let outcome = super::stream_complete(
            &http,
            &hooks,
            &guardrails,
            vec![ChatMessage::text("user", "wake up")],
            &options,
            |_| {},
        )
        .await
        .expect("stream_complete");
        assert_eq!(outcome.content, "ok");

        let requests = server.received_requests().await.expect("requests");
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).expect("json body");
        let msgs = body["messages"].as_array().expect("messages array");
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "system");
        assert_eq!(msgs[2]["content"], "role: check fresno weather");
    }

    #[test]
    fn prompt_prefix_hash_ignores_volatile_suffix() {
        let mut stable = super::ChatOptions::new("https://api.openai.com", "k", "m");
        stable.system_prompt_blocks = Some(vec![super::SystemPromptBlock::cached("base")]);
        let mut with_tail = stable.clone();
        with_tail.volatile_suffix_blocks =
            Some(vec![super::SystemPromptBlock::uncached("todos v2")]);
        let tools = ["read", "write"];
        assert_eq!(
            super::prompt_prefix_hash(&stable, &tools),
            super::prompt_prefix_hash(&with_tail, &tools)
        );
        with_tail.system_prompt_blocks =
            Some(vec![super::SystemPromptBlock::cached("base changed")]);
        assert_ne!(
            super::prompt_prefix_hash(&stable, &tools),
            super::prompt_prefix_hash(&with_tail, &tools)
        );
    }

    #[test]
    fn fail_partial_wraps_cause_with_messages() {
        let messages = vec![
            ChatMessage::text("user", "hello"),
            ChatMessage::text("assistant", "partial"),
        ];
        let err = super::fail_partial(super::ChatError::MaxToolRounds(3), &messages, 0);
        let partial = err.partial_messages().expect("partial messages");
        assert_eq!(partial.len(), 2);
        assert!(matches!(
            err.root_cause(),
            super::ChatError::MaxToolRounds(3)
        ));
        assert_eq!(err.to_string(), "exceeded max tool rounds (3)");
    }

    #[test]
    fn fail_partial_keeps_messages_on_cancel() {
        let messages = vec![
            ChatMessage::text("user", "hello"),
            ChatMessage::text("assistant", "partial"),
        ];
        let err = super::fail_partial(super::ChatError::Cancelled, &messages, 0);
        assert!(err.is_cancelled());
        let partial = err.partial_messages().expect("partial messages");
        assert_eq!(partial.len(), 2);
        assert_eq!(err.to_string(), "request cancelled");
    }

    #[test]
    fn detects_transient_stream_stalls() {
        assert!(super::detail_is_transient_stream_stall(
            "HTTP request failed: error decoding response body"
        ));
        assert!(super::detail_is_transient_stream_stall(
            "operation timed out"
        ));
        assert!(super::detail_is_transient_stream_stall("connection reset"));
        assert!(!super::detail_is_transient_stream_stall(
            "HTTP 400: invalid_request"
        ));
    }

    #[test]
    fn retries_stalled_round_only_before_tokens() {
        let err = super::ChatError::Api("error decoding response body".into());
        assert!(super::should_retry_stalled_stream_round(&err, 0, "", false));
        assert!(super::should_retry_stalled_stream_round(&err, 1, "", false));
        assert!(!super::should_retry_stalled_stream_round(
            &err, 2, "", false
        ));
        assert!(!super::should_retry_stalled_stream_round(
            &err, 0, "hello", false
        ));
        assert!(!super::should_retry_stalled_stream_round(&err, 0, "", true));
        assert!(!super::should_retry_stalled_stream_round(
            &super::ChatError::Cancelled,
            0,
            "",
            false
        ));
    }
}

#[cfg(test)]
mod dispatch_freshness_tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tokio::sync::Mutex;

    use super::{DispatchCtx, dispatch_one, new_tool_loop_guard};
    use crate::events::{CollectingSubscriber, ProcessEventKind, StatusEmitter};
    use crate::hooks::HookRegistry;
    use crate::openai::{FunctionCall, ToolCall};
    use crate::tools::{Tool, ToolInvokeError, ToolRegistry, ToolSpec};

    struct CountingRead {
        calls: AtomicU32,
        content: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl Tool for CountingRead {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("read", json!({"type": "object"}))
        }

        async fn call(&self, _arguments: Value) -> Result<Value, ToolInvokeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let content = self.content.lock().await.clone();
            Ok(json!({ "ok": true, "content": content }))
        }
    }

    struct CountingEdit {
        calls: AtomicU32,
        content: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl Tool for CountingEdit {
        fn spec(&self) -> ToolSpec {
            ToolSpec::new("edit", json!({"type": "object"}))
        }

        async fn call(&self, arguments: Value) -> Result<Value, ToolInvokeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let new_string = arguments
                .get("new_string")
                .and_then(Value::as_str)
                .unwrap_or("edited")
                .to_string();
            *self.content.lock().await = new_string.clone();
            Ok(json!({ "ok": true, "rel_path": "src/foo.rs" }))
        }
    }

    fn tool_call(id: &str, name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: args.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn read_edit_read_invokes_read_twice_and_returns_post_edit_state() {
        let content = Arc::new(Mutex::new("before".to_string()));
        let read = Arc::new(CountingRead {
            calls: AtomicU32::new(0),
            content: Arc::clone(&content),
        });
        let edit = Arc::new(CountingEdit {
            calls: AtomicU32::new(0),
            content: Arc::clone(&content),
        });
        let registry = ToolRegistry::new();
        registry
            .register(read.clone())
            .await
            .expect("register read");
        registry
            .register(edit.clone())
            .await
            .expect("register edit");
        let hooks = HookRegistry::new();
        let loop_guard = new_tool_loop_guard();
        let ctx = DispatchCtx {
            hooks: &hooks,
            registry: &registry,
            status_emitter: None,
            request_id: "req",
            round: 1,
            model: "test",
            emit_start: false,
            tool_result_max_chars: 0,
            loop_guard: Some(&loop_guard),
            cancel: None,
            code_allowlist: None,
        };

        let first_read = dispatch_one(
            &tool_call("1", "read", json!({"path": "src/foo.rs"})),
            ctx.clone(),
        )
        .await
        .expect("first read");
        let first_text = first_read
            .content
            .as_ref()
            .and_then(|content| content.as_text())
            .expect("first read text");
        assert!(first_text.contains("before"));

        dispatch_one(
            &tool_call(
                "2",
                "edit",
                json!({
                    "path": "src/foo.rs",
                    "old_string": "before",
                    "new_string": "after"
                }),
            ),
            ctx.clone(),
        )
        .await
        .expect("edit");

        let second_read = dispatch_one(&tool_call("3", "read", json!({"path": "src/foo.rs"})), ctx)
            .await
            .expect("second read");
        let second_text = second_read
            .content
            .as_ref()
            .and_then(|content| content.as_text())
            .expect("second read text");
        assert!(
            second_text.contains("after"),
            "second read must return post-edit state, got {second_text}"
        );
        assert!(!second_text.contains("before"));
        assert_eq!(read.calls.load(Ordering::SeqCst), 2);
        assert_eq!(edit.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn tool_call_start_event_includes_arguments() {
        let read = Arc::new(CountingRead {
            calls: AtomicU32::new(0),
            content: Arc::new(Mutex::new("body".to_string())),
        });
        let registry = ToolRegistry::new();
        registry.register(read).await.expect("register read");
        let hooks = HookRegistry::new();
        let emitter = Arc::new(StatusEmitter::new());
        let collector = Arc::new(CollectingSubscriber::new());
        emitter.subscribe(collector.clone()).await;
        let ctx = DispatchCtx {
            hooks: &hooks,
            registry: &registry,
            status_emitter: Some(&emitter),
            request_id: "req",
            round: 1,
            model: "test",
            emit_start: true,
            tool_result_max_chars: 0,
            loop_guard: None,
            cancel: None,
            code_allowlist: None,
        };
        let args = json!({"path": "src/foo.rs"}).to_string();
        dispatch_one(&tool_call("1", "read", json!({"path": "src/foo.rs"})), ctx)
            .await
            .expect("read");
        let events = collector.take_events().await;
        let start = events
            .iter()
            .find(|event| event.kind == ProcessEventKind::ToolCallStart)
            .expect("tool start");
        assert_eq!(
            start.metadata.get("arguments").map(String::as_str),
            Some(args.as_str())
        );
        let end = events
            .iter()
            .find(|event| event.kind == ProcessEventKind::ToolCallEnd)
            .expect("tool end");
        assert!(
            end.metadata
                .get("result")
                .is_some_and(|result| result.contains("body"))
        );
    }

    #[test]
    fn chat_rejects_an_embedding_model_reference() {
        let model_ref =
            crate::providers::parse_model_ref("openai:embedding:text-embedding-3-small");
        let Err(error) =
            super::resolve_chat_provider(&model_ref, &crate::providers::ProviderCredentials::new())
        else {
            panic!("embedding model must not resolve as chat");
        };
        assert!(matches!(
            error,
            super::ChatError::UnsupportedModelCapability(
                crate::providers::ModelCapability::Embedding
            )
        ));
    }

    #[tokio::test]
    async fn gateway_streams_anthropic_via_openai_sse_in_tool_loop() {
        use std::sync::Arc;
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use crate::guardrails::GuardrailRegistry;
        use crate::hooks::HookRegistry;
        use crate::http::{ClientConfig, HttpClient};
        use crate::openai::ChatMessage;
        use crate::providers::ProviderCredentials;
        use crate::tools::ToolRegistry;

        let server = MockServer::start().await;
        let sse = concat!(
            "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello from gateway\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sgw-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mut creds = ProviderCredentials::new();
        creds.set_gateway(server.uri(), "sgw-test");
        let mut options = super::ChatOptions::new("", "", "anthropic:claude-sonnet-4-5");
        options.provider_credentials = Some(Arc::new(creds));

        let http = HttpClient::new(ClientConfig::default()).expect("http");
        let tools = ToolRegistry::new();
        let hooks = HookRegistry::new();
        let guardrails = GuardrailRegistry::new();
        let mut chunks = String::new();

        let outcome = super::stream_complete_with_tools(
            &http,
            &tools,
            &hooks,
            &guardrails,
            vec![ChatMessage::text("user", "hi")],
            &options,
            |delta| chunks.push_str(&delta),
            |_| {},
        )
        .await
        .expect("gateway anthropic stream");

        assert_eq!(outcome.content, "hello from gateway");
        assert_eq!(chunks, "hello from gateway");
    }
}
