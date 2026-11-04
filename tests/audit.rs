//! Unit tests for `superglue::audit` — MemoryRunStore and RunRecorder.

use std::sync::Arc;

use superglue::audit::{AuditError, AuditStore, JsonlRunStore, MemoryRunStore, RunRecorder};
use superglue::hooks::{HookContext, HookHandler, HookStage};
use superglue::proto;

// ---------------------------------------------------------------------------
// MemoryRunStore tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runstore_insert_and_get() {
    let store = MemoryRunStore::new(None);
    let record = proto::RunRecord {
        request_id: "req-1".to_string(),
        messages: vec![],
        hook_events: vec![],
        outcome: None,
        started_at_ms: 1000,
        finished_at_ms: 2000,
        process_events: vec![],
        options_json: None,
        model_used: None,
        previous_response_id: None,
    };
    store.insert(record).await.unwrap();

    let found = store.get("req-1").await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().request_id, "req-1");
}

#[tokio::test]
async fn runstore_get_missing_returns_none() {
    let store = MemoryRunStore::new(None);
    assert!(store.get("nonexistent").await.is_none());
}

#[tokio::test]
async fn runstore_list_all() {
    let store = MemoryRunStore::new(None);
    for i in 0..3 {
        store
            .insert(proto::RunRecord {
                request_id: format!("req-{i}"),
                messages: vec![],
                hook_events: vec![],
                outcome: None,
                started_at_ms: i as i64,
                finished_at_ms: i as i64 + 1,
                process_events: vec![],
                options_json: None,
                model_used: None,
                previous_response_id: None,
            })
            .await
            .unwrap();
    }
    let all = store.list().await;
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].request_id, "req-0");
    assert_eq!(all[2].request_id, "req-2");
}

#[tokio::test]
async fn runstore_clear() {
    let store = MemoryRunStore::new(None);
    store
        .insert(proto::RunRecord {
            request_id: "r".to_string(),
            messages: vec![],
            hook_events: vec![],
            outcome: None,
            started_at_ms: 0,
            finished_at_ms: 1,
            process_events: vec![],
            options_json: None,
            model_used: None,
            previous_response_id: None,
        })
        .await
        .unwrap();
    assert_eq!(store.len().await, 1);
    store.clear().await;
    assert_eq!(store.len().await, 0);
    assert!(store.is_empty().await);
}

#[tokio::test]
async fn runstore_capacity_cap_evicts_oldest() {
    let store = MemoryRunStore::new(Some(3));
    for i in 0..5u32 {
        store
            .insert(proto::RunRecord {
                request_id: format!("req-{i}"),
                messages: vec![],
                hook_events: vec![],
                outcome: None,
                started_at_ms: i as i64,
                finished_at_ms: i as i64 + 1,
                process_events: vec![],
                options_json: None,
                model_used: None,
                previous_response_id: None,
            })
            .await
            .unwrap();
    }
    // Only the 3 most recently inserted records should remain.
    assert_eq!(store.len().await, 3);
    let all = store.list().await;
    let ids: Vec<&str> = all.iter().map(|r| r.request_id.as_str()).collect();
    assert!(!ids.contains(&"req-0"), "req-0 should have been evicted");
    assert!(!ids.contains(&"req-1"), "req-1 should have been evicted");
    assert!(ids.contains(&"req-4"), "req-4 should be present");
}

#[tokio::test]
async fn runrecorder_caps_hook_events() {
    let store = Arc::new(MemoryRunStore::new(None));
    let recorder = RunRecorder::with_caps(store.clone(), Some(2), Some(2));

    for i in 0..5 {
        recorder
            .execute(HookContext::new(HookStage::OnRetry, format!("retry-{i}")))
            .await
            .unwrap();
    }

    assert_eq!(recorder.peek_events().await.len(), 2);
}

// ---------------------------------------------------------------------------
// RunRecorder hook-handler tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runrecorder_accumulates_events_on_execute() {
    let store = Arc::new(MemoryRunStore::new(None));
    let recorder = RunRecorder::new(store.clone());

    // Simulate two hook calls.
    let ctx1 = HookContext::new(HookStage::PreCompletion, "hello");
    let ctx2 = HookContext::with_meta(HookStage::PreTool, "{}", "tool_name", "echo");

    recorder.execute(ctx1).await.unwrap();
    recorder.execute(ctx2).await.unwrap();

    let events = recorder.peek_events().await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].stage, "pre_completion");
    assert_eq!(events[1].stage, "pre_tool");
    assert_eq!(
        events[1].metadata.get("tool_name").map(|s| s.as_str()),
        Some("echo")
    );
}

#[tokio::test]
async fn runrecorder_finish_stores_record_and_clears_events() {
    let store = Arc::new(MemoryRunStore::new(None));
    let recorder = RunRecorder::new(store.clone());

    recorder
        .execute(HookContext::new(HookStage::PostCompletion, "hi"))
        .await
        .unwrap();

    let outcome = proto::CompletionOutcome {
        content: Some("hi".into()),
        rounds: 1,
        usage: None,
        finish_reason: Some("stop".into()),
    };

    recorder
        .finish("req-abc".to_string(), vec![], outcome, 1000, 2000)
        .await;

    // The record should now be in the store.
    let record = store.get("req-abc").await.unwrap();
    assert_eq!(record.request_id, "req-abc");
    assert_eq!(record.hook_events.len(), 1);
    assert_eq!(record.hook_events[0].stage, "post_completion");
    assert_eq!(record.started_at_ms, 1000);
    assert_eq!(record.finished_at_ms, 2000);

    // The recorder should have cleared its internal buffer after flushing.
    assert!(
        recorder.peek_events().await.is_empty(),
        "events not cleared after finish"
    );
}

#[tokio::test]
async fn runrecorder_all_stages_returns_seven_variants() {
    let stages = RunRecorder::all_stages();
    assert_eq!(stages.len(), 7);
}

#[tokio::test]
async fn resume_context_restores_messages_and_options() {
    let options_json = superglue::audit::options_snapshot(&superglue::chat::ChatOptions {
        model: "openai:gpt-4o-mini".into(),
        ..Default::default()
    });
    let record = proto::RunRecord {
        request_id: "req-resume".into(),
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
        model_used: Some("openai:gpt-4o-mini".into()),
        previous_response_id: Some("resp_123".into()),
    };
    let ctx = superglue::audit::resume_context_from_record(
        &record,
        superglue::chat::ChatOptions::default(),
    )
    .unwrap();
    assert_eq!(ctx.options.model, "openai:gpt-4o-mini");
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.previous_response_id.as_deref(), Some("resp_123"));
}

#[tokio::test]
async fn runrecorder_hook_events_include_timestamp() {
    let store = Arc::new(MemoryRunStore::new(None));
    let recorder = RunRecorder::new(store.clone());

    recorder
        .execute(HookContext::new(HookStage::OnRetry, "retry"))
        .await
        .unwrap();

    let events = recorder.peek_events().await;
    assert!(events[0].timestamp_ms > 0, "timestamp should be positive");
}

// ---------------------------------------------------------------------------
// JsonlRunStore tests
// ---------------------------------------------------------------------------

const RUN: &str = "run-1";

fn step(
    kind: proto::WorkflowTransitionKind,
    message: Option<proto::ChatMessage>,
) -> proto::WorkflowTransition {
    proto::WorkflowTransition {
        request_id: RUN.into(),
        kind: kind as i32,
        timestamp_ms: 1,
        message,
        ..Default::default()
    }
}

fn started() -> proto::WorkflowTransition {
    proto::WorkflowTransition {
        options_json: Some(r#"{"model":"openai:gpt-4o-mini","max_tool_rounds":4}"#.into()),
        ..step(proto::WorkflowTransitionKind::RunStarted, None)
    }
}

fn appended(message: proto::ChatMessage) -> proto::WorkflowTransition {
    step(
        proto::WorkflowTransitionKind::MessageAppended,
        Some(message),
    )
}

fn text(role: &str, content: &str) -> proto::ChatMessage {
    proto::ChatMessage {
        role: role.into(),
        content: Some(content.into()),
        ..Default::default()
    }
}

fn tool_calls(ids: &[&str]) -> proto::ChatMessage {
    proto::ChatMessage {
        role: "assistant".into(),
        tool_calls: ids
            .iter()
            .map(|id| proto::ToolCall {
                id: (*id).into(),
                kind: "function".into(),
                function: Some(proto::FunctionCall {
                    name: "lookup".into(),
                    arguments: "{}".into(),
                }),
            })
            .collect(),
        ..Default::default()
    }
}

fn tool_result(id: &str) -> proto::ChatMessage {
    proto::ChatMessage {
        role: "tool".into(),
        content: Some("ok".into()),
        tool_call_id: Some(id.into()),
        ..Default::default()
    }
}

fn roles(record: &proto::RunRecord) -> Vec<&str> {
    record.messages.iter().map(|m| m.role.as_str()).collect()
}

async fn append_all(store: &JsonlRunStore, steps: Vec<proto::WorkflowTransition>) {
    for transition in steps {
        store.append(transition).await.unwrap();
    }
}

#[tokio::test]
async fn jsonl_partial_run_survives_reopen_and_resumes() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = JsonlRunStore::open(dir.path()).unwrap();
        append_all(
            &store,
            vec![
                started(),
                appended(text("user", "find it")),
                appended(tool_calls(&["c1"])),
                appended(tool_result("c1")),
            ],
        )
        .await;
    }

    let store = JsonlRunStore::open(dir.path()).unwrap();
    let record = store.get(RUN).await.expect("partial run replays");
    assert_eq!(roles(&record), ["user", "assistant", "tool"]);
    assert!(record.outcome.is_none());

    let ctx = superglue::audit::resume_context_from_record(
        &record,
        superglue::chat::ChatOptions::default(),
    )
    .expect("options_json from run_started");
    assert_eq!(ctx.options.model, "openai:gpt-4o-mini");
    assert_eq!(ctx.messages.len(), 3);
}

#[tokio::test]
async fn jsonl_skips_truncated_last_line() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonlRunStore::open(dir.path()).unwrap();
    append_all(&store, vec![started(), appended(text("user", "hi"))]).await;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join(format!("{RUN}.jsonl")))
        .unwrap();
    write!(file, r#"{{"request_id":"run-1","kind":"message_app"#).unwrap();

    let record = store.get(RUN).await.expect("good lines still replay");
    assert_eq!(roles(&record), ["user"]);
}

#[tokio::test]
async fn jsonl_rejects_path_like_request_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonlRunStore::open(dir.path().join("audit")).unwrap();
    let mut transition = started();
    transition.request_id = "../escape".into();

    let error = store.append(transition).await.unwrap_err();
    assert!(matches!(error, AuditError::BadRequestId));
    assert!(store.get("../escape").await.is_none());
    assert!(!dir.path().join("escape.jsonl").exists());
}

#[tokio::test]
async fn jsonl_drops_incomplete_tool_round() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonlRunStore::open(dir.path()).unwrap();
    append_all(
        &store,
        vec![
            started(),
            appended(text("user", "do two things")),
            appended(tool_calls(&["c1", "c2"])),
            appended(tool_result("c1")),
        ],
    )
    .await;

    let record = store.get(RUN).await.unwrap();
    assert_eq!(roles(&record), ["user"], "the unfinished round runs again");
}

#[tokio::test]
async fn jsonl_run_started_replaces_earlier_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonlRunStore::open(dir.path()).unwrap();
    append_all(
        &store,
        vec![
            started(),
            appended(text("user", "first")),
            started(),
            appended(text("user", "first")),
            appended(text("assistant", "done")),
        ],
    )
    .await;

    let record = store.get(RUN).await.unwrap();
    assert_eq!(roles(&record), ["user", "assistant"]);
}

#[tokio::test]
async fn jsonl_insert_writes_record_without_a_step_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonlRunStore::open(dir.path()).unwrap();
    store
        .insert(proto::RunRecord {
            request_id: RUN.into(),
            messages: vec![text("user", "hi"), text("assistant", "hello")],
            outcome: Some(proto::CompletionOutcome {
                content: Some("hello".into()),
                rounds: 1,
                usage: None,
                finish_reason: Some("stop".into()),
            }),
            started_at_ms: 5,
            finished_at_ms: 6,
            ..Default::default()
        })
        .await
        .unwrap();

    let listed = store.list().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(roles(&listed[0]), ["user", "assistant"]);
    assert_eq!(listed[0].finished_at_ms, 6);
}

#[cfg(unix)]
#[tokio::test]
async fn jsonl_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("audit");
    let store = JsonlRunStore::open(&root).unwrap();
    store.append(started()).await.unwrap();

    let dir_mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
    let file_mode = std::fs::metadata(root.join(format!("{RUN}.jsonl")))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    assert_eq!(file_mode, 0o600);
}

#[tokio::test]
async fn memory_store_folds_partial_run_until_insert() {
    let store = MemoryRunStore::new(None);
    store.append(started()).await.unwrap();
    store.append(appended(text("user", "hi"))).await.unwrap();
    assert_eq!(roles(&store.get(RUN).await.unwrap()), ["user"]);
    assert_eq!(store.len().await, 0);
}
