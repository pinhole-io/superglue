//! Opt-in sink for replayable tool-loop steps.

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;

use crate::openai::ChatMessage;

/// One step the unary tool loop reports. The audit recorder writes the terminal steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    RunStarted,
    MessageAppended,
}

/// A step handed to the audit log.
#[derive(Debug, Clone)]
pub struct TransitionNote {
    pub request_id: String,
    pub kind: TransitionKind,
    pub message: Option<ChatMessage>,
}

type RecordFn = dyn Fn(TransitionNote) -> BoxFuture<'static, ()> + Send + Sync;

/// Opt-in sink for replayable tool-loop steps.
///
/// The loop awaits each note, so notes for one run arrive in order.
/// The sink logs an append failure. The completion still returns.
#[derive(Clone)]
pub struct TransitionLog(Arc<RecordFn>);

impl TransitionLog {
    pub fn new(
        record: impl Fn(TransitionNote) -> BoxFuture<'static, ()> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(record))
    }

    pub async fn record(&self, note: TransitionNote) {
        (self.0)(note).await;
    }
}

impl fmt::Debug for TransitionLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TransitionLog(..)")
    }
}

/// Record `RunStarted` followed by one `MessageAppended` per message.
pub(crate) async fn emit_start(
    log: Option<&TransitionLog>,
    request_id: &str,
    messages: &[ChatMessage],
) {
    let Some(log) = log else {
        return;
    };
    log.record(note(request_id, TransitionKind::RunStarted, None))
        .await;
    emit_messages(Some(log), request_id, messages).await;
}

/// Record one `MessageAppended` per message, in order.
pub(crate) async fn emit_messages(
    log: Option<&TransitionLog>,
    request_id: &str,
    messages: &[ChatMessage],
) {
    let Some(log) = log else {
        return;
    };
    for message in messages {
        log.record(note(
            request_id,
            TransitionKind::MessageAppended,
            Some(message.clone()),
        ))
        .await;
    }
}

fn note(request_id: &str, kind: TransitionKind, message: Option<ChatMessage>) -> TransitionNote {
    TransitionNote {
        request_id: request_id.to_string(),
        kind,
        message,
    }
}
