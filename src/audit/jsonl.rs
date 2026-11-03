//! Opt-in JSONL audit log. One file per request id.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};
use tokio::sync::RwLock;

use super::{AuditError, record_from_transitions};
use crate::proto;

/// Directory of `{request_id}.jsonl` transition logs.
pub struct JsonlRunStore {
    dir: PathBuf,
    write_lock: Mutex<()>,
    seqs: Mutex<HashMap<String, u32>>,
    records: RwLock<HashMap<String, proto::RunRecord>>,
}

impl JsonlRunStore {
    /// Create `dir` when it is missing. Files in the directory are mode `0600`.
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
            write_lock: Mutex::new(()),
            seqs: Mutex::new(HashMap::new()),
            records: RwLock::new(HashMap::new()),
        })
    }

    pub fn load(&self, request_id: &str) -> Result<proto::RunRecord, AuditError> {
        let transitions = self.read_transitions(request_id)?;
        Ok(record_from_transitions(&transitions))
    }

    fn path_for(&self, request_id: &str) -> Result<PathBuf, AuditError> {
        validate_request_id(request_id)?;
        Ok(self.dir.join(format!("{request_id}.jsonl")))
    }

    fn read_transitions(
        &self,
        request_id: &str,
    ) -> Result<Vec<proto::WorkflowTransition>, AuditError> {
        let path = self.path_for(request_id)?;
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        let reader = BufReader::new(file);
        let mut lines = Vec::new();
        for line in reader.lines() {
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
}

impl super::AuditStore for JsonlRunStore {
    fn append_blocking(&self, mut transition: proto::WorkflowTransition) -> Result<(), AuditError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| AuditError::Poisoned)?;
        transition.seq = next_seq(&self.seqs, &transition.request_id)?;
        let path = self.path_for(&transition.request_id)?;
        let mut file = open_private(&path)?;
        writeln!(file, "{}", transition_to_json(&transition))?;
        file.sync_all()?;
        Ok(())
    }

    async fn insert(&self, record: proto::RunRecord) -> Result<(), AuditError> {
        self.records
            .write()
            .await
            .insert(record.request_id.clone(), record);
        Ok(())
    }

    async fn get(&self, request_id: &str) -> Option<proto::RunRecord> {
        if let Some(record) = self.records.read().await.get(request_id).cloned() {
            return Some(record);
        }
        self.load(request_id).ok().filter(|record| {
            !record.request_id.is_empty() || !record.messages.is_empty()
        })
    }

    async fn list(&self) -> Vec<proto::RunRecord> {
        let mut records: Vec<_> = self.records.read().await.values().cloned().collect();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(_) => return records,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(request_id) = name.strip_suffix(".jsonl") else {
                continue;
            };
            if records.iter().any(|record| record.request_id == request_id) {
                continue;
            }
            if let Ok(record) = self.load(request_id) {
                records.push(record);
            }
        }
        records
    }
}

pub(crate) fn next_seq(
    seqs: &Mutex<HashMap<String, u32>>,
    request_id: &str,
) -> Result<u32, AuditError> {
    let mut guard = seqs.lock().map_err(|_| AuditError::Poisoned)?;
    let seq = guard.entry(request_id.to_string()).or_insert(0);
    let current = *seq;
    *seq = current.saturating_add(1);
    Ok(current)
}

pub(crate) fn validate_request_id(request_id: &str) -> Result<(), AuditError> {
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

fn open_private(path: &Path) -> Result<fs::File, AuditError> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub(crate) fn transition_to_json(transition: &proto::WorkflowTransition) -> String {
    json!({
        "request_id": transition.request_id,
        "seq": transition.seq,
        "kind": kind_name(transition.kind),
        "timestamp_ms": transition.timestamp_ms,
        "message": transition.message.as_ref().map(message_to_json),
        "outcome": transition.outcome.as_ref().map(outcome_to_json),
        "error_type": transition.error_type,
        "previous_response_id": transition.previous_response_id,
    })
    .to_string()
}

pub(crate) fn transition_from_json(line: &str) -> Result<proto::WorkflowTransition, AuditError> {
    let value: Value = serde_json::from_str(line)?;
    let request_id = value
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    validate_request_id(&request_id)?;
    let kind = kind_from_name(value.get("kind").and_then(Value::as_str).unwrap_or(""));
    Ok(proto::WorkflowTransition {
        request_id,
        seq: value.get("seq").and_then(Value::as_u64).unwrap_or(0) as u32,
        kind,
        timestamp_ms: value.get("timestamp_ms").and_then(Value::as_i64).unwrap_or(0),
        message: value.get("message").and_then(message_from_json),
        process_event: None,
        outcome: value.get("outcome").and_then(outcome_from_json),
        error_type: value
            .get("error_type")
            .and_then(Value::as_str)
            .map(str::to_string),
        previous_response_id: value
            .get("previous_response_id")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn kind_name(kind: i32) -> &'static str {
    match proto::WorkflowTransitionKind::try_from(kind) {
        Ok(proto::WorkflowTransitionKind::RunStarted) => "run_started",
        Ok(proto::WorkflowTransitionKind::MessageAppended) => "message_appended",
        Ok(proto::WorkflowTransitionKind::LlmCallFinished) => "llm_call_finished",
        Ok(proto::WorkflowTransitionKind::ToolCallFinished) => "tool_call_finished",
        Ok(proto::WorkflowTransitionKind::RunFinished) => "run_finished",
        Ok(proto::WorkflowTransitionKind::RunFailed) => "run_failed",
        _ => "run_started",
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
                    id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    kind: call
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("function")
                        .to_string(),
                    function: Some(proto::FunctionCall {
                        name: call
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        arguments: call
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}")
                            .to_string(),
                    }),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(proto::ChatMessage {
        role: value
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        content: value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_calls,
        tool_call_id: value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        name: value.get("name").and_then(Value::as_str).map(str::to_string),
        refusal: value
            .get("refusal")
            .and_then(Value::as_str)
            .map(str::to_string),
        provider_blocks_json: value
            .get("provider_blocks_json")
            .and_then(Value::as_str)
            .map(str::to_string),
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
        content: value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string),
        rounds: value.get("rounds").and_then(Value::as_u64).unwrap_or(0) as u32,
        usage: None,
        finish_reason: value
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// Fold transitions into a record and drop a trailing assistant tool call that has no results.
pub fn fold_transitions(transitions: &[proto::WorkflowTransition]) -> proto::RunRecord {
    record_from_transitions(transitions)
}
