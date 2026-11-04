//! Opt-in JSONL audit log. One file per request id.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{AuditError, AuditStore, record_from_transitions, record_to_transitions};
use crate::proto;

/// Directory of `{request_id}.jsonl` step logs.
///
/// The file is the source of truth. The store keeps no records in memory and
/// never deletes files. The caller removes old files.
#[derive(Debug)]
pub struct JsonlRunStore {
    dir: PathBuf,
    seqs: Mutex<HashMap<String, u32>>,
}

impl JsonlRunStore {
    /// Create `dir` as mode `0700` when it is missing. Files are mode `0600`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, AuditError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            dir,
            seqs: Mutex::new(HashMap::new()),
        })
    }

    /// Read `{dir}/{request_id}.jsonl` back into a record. This call blocks on file I/O.
    pub fn load(&self, request_id: &str) -> Result<proto::RunRecord, AuditError> {
        let transitions = read_transitions(&path_for(&self.dir, request_id)?)?;
        Ok(record_from_transitions(&transitions))
    }

    fn next_seq(&self, request_id: &str, kind: proto::WorkflowTransitionKind) -> u32 {
        let mut guard = self
            .seqs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match kind {
            proto::WorkflowTransitionKind::RunFinished
            | proto::WorkflowTransitionKind::RunFailed => guard.remove(request_id).unwrap_or(0),
            _ => {
                let seq = guard.entry(request_id.to_string()).or_insert(0);
                let current = *seq;
                *seq = current.saturating_add(1);
                current
            }
        }
    }
}

#[async_trait]
impl AuditStore for JsonlRunStore {
    async fn append(&self, mut transition: proto::WorkflowTransition) -> Result<(), AuditError> {
        let path = path_for(&self.dir, &transition.request_id)?;
        let kind = proto::WorkflowTransitionKind::try_from(transition.kind)
            .unwrap_or(proto::WorkflowTransitionKind::RunStarted);
        transition.seq = self.next_seq(&transition.request_id, kind);
        let line = transition_to_json(&transition);
        blocking(move || append_lines(&path, std::slice::from_ref(&line))).await
    }

    /// Write the record as steps when no step file exists yet.
    ///
    /// A run recorded through a transition log already has its file.
    async fn insert(&self, record: proto::RunRecord) -> Result<(), AuditError> {
        let path = path_for(&self.dir, &record.request_id)?;
        let lines: Vec<String> = record_to_transitions(&record)
            .iter()
            .map(transition_to_json)
            .collect();
        blocking(move || {
            if path.exists() {
                return Ok(());
            }
            append_lines(&path, &lines)
        })
        .await
    }

    async fn get(&self, request_id: &str) -> Option<proto::RunRecord> {
        let path = path_for(&self.dir, request_id).ok()?;
        let transitions = blocking(move || read_transitions(&path)).await.ok()?;
        if transitions.is_empty() {
            return None;
        }
        Some(record_from_transitions(&transitions))
    }

    async fn list(&self) -> Vec<proto::RunRecord> {
        let dir = self.dir.clone();
        blocking(move || Ok(list_records(&dir)))
            .await
            .unwrap_or_default()
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AuditError> + Send + 'static,
) -> Result<T, AuditError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| AuditError::Io(std::io::Error::other(error)))?
}

fn list_records(dir: &Path) -> Vec<proto::RunRecord> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(request_id) = name.to_str().and_then(|name| name.strip_suffix(".jsonl")) else {
            continue;
        };
        let Ok(path) = path_for(dir, request_id) else {
            continue;
        };
        match read_transitions(&path) {
            Ok(transitions) if !transitions.is_empty() => {
                records.push(record_from_transitions(&transitions));
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("skipping unreadable audit log: {error}"),
        }
    }
    records.sort_by_key(|record| record.started_at_ms);
    records
}

fn path_for(dir: &Path, request_id: &str) -> Result<PathBuf, AuditError> {
    validate_request_id(request_id)?;
    Ok(dir.join(format!("{request_id}.jsonl")))
}

/// Read every step. A bad final line is a torn write and is skipped.
/// A bad line before the end is corruption and fails the read.
fn read_transitions(path: &Path) -> Result<Vec<proto::WorkflowTransition>, AuditError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut lines = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    let mut transitions = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        match transition_from_json(line) {
            Ok(transition) => transitions.push(transition),
            Err(error) if index + 1 == lines.len() => {
                tracing::warn!("skipping truncated audit line: {error}");
            }
            Err(error) => return Err(error),
        }
    }
    Ok(transitions)
}

fn append_lines(path: &Path, lines: &[String]) -> Result<(), AuditError> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    for line in lines {
        writeln!(file, "{line}")?;
    }
    file.sync_all()?;
    Ok(())
}

fn validate_request_id(request_id: &str) -> Result<(), AuditError> {
    if !request_id.is_empty()
        && request_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        Ok(())
    } else {
        Err(AuditError::BadRequestId)
    }
}

fn transition_to_json(transition: &proto::WorkflowTransition) -> String {
    json!({
        "request_id": transition.request_id,
        "seq": transition.seq,
        "kind": kind_name(transition.kind),
        "timestamp_ms": transition.timestamp_ms,
        "message": transition.message.as_ref().map(message_to_json),
        "outcome": transition.outcome.as_ref().map(outcome_to_json),
        "error_type": transition.error_type,
        "previous_response_id": transition.previous_response_id,
        "options_json": transition.options_json,
    })
    .to_string()
}

fn transition_from_json(line: &str) -> Result<proto::WorkflowTransition, AuditError> {
    let value: Value = serde_json::from_str(line)?;
    let request_id = optional_string(&value, "request_id").unwrap_or_default();
    validate_request_id(&request_id)?;
    Ok(proto::WorkflowTransition {
        request_id,
        seq: value
            .get("seq")
            .and_then(Value::as_u64)
            .and_then(|seq| u32::try_from(seq).ok())
            .unwrap_or(0),
        kind: kind_from_name(value.get("kind").and_then(Value::as_str).unwrap_or("")),
        timestamp_ms: value
            .get("timestamp_ms")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        message: value.get("message").and_then(message_from_json),
        process_event: None,
        outcome: value.get("outcome").and_then(outcome_from_json),
        error_type: optional_string(&value, "error_type"),
        previous_response_id: optional_string(&value, "previous_response_id"),
        options_json: optional_string(&value, "options_json"),
    })
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn kind_name(kind: i32) -> &'static str {
    match proto::WorkflowTransitionKind::try_from(kind) {
        Ok(proto::WorkflowTransitionKind::MessageAppended) => "message_appended",
        Ok(proto::WorkflowTransitionKind::LlmCallFinished) => "llm_call_finished",
        Ok(proto::WorkflowTransitionKind::ToolCallFinished) => "tool_call_finished",
        Ok(proto::WorkflowTransitionKind::RunFinished) => "run_finished",
        Ok(proto::WorkflowTransitionKind::RunFailed) => "run_failed",
        Ok(proto::WorkflowTransitionKind::RunStarted) | Err(_) => "run_started",
    }
}

fn kind_from_name(name: &str) -> i32 {
    let kind = match name {
        "message_appended" => proto::WorkflowTransitionKind::MessageAppended,
        "llm_call_finished" => proto::WorkflowTransitionKind::LlmCallFinished,
        "tool_call_finished" => proto::WorkflowTransitionKind::ToolCallFinished,
        "run_finished" => proto::WorkflowTransitionKind::RunFinished,
        "run_failed" => proto::WorkflowTransitionKind::RunFailed,
        _ => proto::WorkflowTransitionKind::RunStarted,
    };
    kind as i32
}

fn message_to_json(message: &proto::ChatMessage) -> Value {
    json!({
        "role": message.role,
        "content": message.content,
        "tool_calls": message.tool_calls.iter().map(|call| {
            json!({
                "id": call.id,
                "kind": call.kind,
                "name": call.function.as_ref().map(|function| function.name.clone()),
                "arguments": call.function.as_ref().map(|function| function.arguments.clone()),
            })
        }).collect::<Vec<_>>(),
        "tool_call_id": message.tool_call_id,
        "name": message.name,
        "refusal": message.refusal,
        "provider_blocks_json": message.provider_blocks_json,
    })
}

fn message_from_json(value: &Value) -> Option<proto::ChatMessage> {
    if value.is_null() {
        return None;
    }
    let tool_calls = value
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| {
            calls
                .iter()
                .map(|call| proto::ToolCall {
                    id: optional_string(call, "id").unwrap_or_default(),
                    kind: optional_string(call, "kind").unwrap_or_else(|| "function".to_string()),
                    function: Some(proto::FunctionCall {
                        name: optional_string(call, "name").unwrap_or_default(),
                        arguments: optional_string(call, "arguments")
                            .unwrap_or_else(|| "{}".to_string()),
                    }),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(proto::ChatMessage {
        role: optional_string(value, "role").unwrap_or_default(),
        content: optional_string(value, "content"),
        tool_calls,
        tool_call_id: optional_string(value, "tool_call_id"),
        name: optional_string(value, "name"),
        refusal: optional_string(value, "refusal"),
        provider_blocks_json: optional_string(value, "provider_blocks_json"),
    })
}

fn outcome_to_json(outcome: &proto::CompletionOutcome) -> Value {
    json!({
        "content": outcome.content,
        "rounds": outcome.rounds,
        "finish_reason": outcome.finish_reason,
    })
}

fn outcome_from_json(value: &Value) -> Option<proto::CompletionOutcome> {
    if value.is_null() {
        return None;
    }
    Some(proto::CompletionOutcome {
        content: optional_string(value, "content"),
        rounds: value
            .get("rounds")
            .and_then(Value::as_u64)
            .and_then(|rounds| u32::try_from(rounds).ok())
            .unwrap_or(0),
        usage: None,
        finish_reason: optional_string(value, "finish_reason"),
    })
}
