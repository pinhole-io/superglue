//! Opt-in sink for replayable tool-loop steps.

use std::fmt;
use std::sync::Arc;

use crate::openai::ChatMessage;

/// One recorded step of a unary tool loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    RunStarted,
    MessageAppended,
    LlmCallFinished,
    ToolCallFinished,
    RunFinished,
    RunFailed,
}

/// A message or terminal marker handed to the audit log.
#[derive(Debug, Clone)]
pub struct TransitionNote {
    pub request_id: String,
    pub kind: TransitionKind,
    pub message: Option<ChatMessage>,
    pub error_type: Option<String>,
    pub previous_response_id: Option<String>,
}

/// Opt-in sink for replayable tool-loop steps.
///
/// The callback logs an append failure. The completion still returns.
#[derive(Clone)]
pub struct TransitionLog(Arc<dyn Fn(TransitionNote) + Send + Sync>);

impl TransitionLog {
    pub fn new(record: impl Fn(TransitionNote) + Send + Sync + 'static) -> Self {
        Self(Arc::new(record))
    }

    pub fn record(&self, note: TransitionNote) {
        (self.0)(note);
    }
}

impl fmt::Debug for TransitionLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransitionLog(..)")
    }
}

pub(crate) fn emit(
    log: Option<&TransitionLog>,
    request_id: &str,
    kind: TransitionKind,
    message: Option<&ChatMessage>,
    error_type: Option<&str>,
    previous_response_id: Option<&str>,
) {
    let Some(log) = log else {
        return;
    };
    log.record(TransitionNote {
        request_id: request_id.to_string(),
        kind,
        message: message.cloned(),
        error_type: error_type.map(str::to_string),
        previous_response_id: previous_response_id.map(str::to_string),
    });
}
