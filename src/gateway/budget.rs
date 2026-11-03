//! Budget enforcement helpers.

use crate::costing::estimate_model_call_cost_usd;
use crate::gateway::db::Database;
use crate::openai::ChatMessage;
use crate::proto;

/// A live budget hold. Drop releases it when the caller did not settle the row.
pub struct BudgetGuard {
    db: Option<Database>,
    request_id: String,
}

impl BudgetGuard {
    pub(crate) fn arm(db: Database, request_id: impl Into<String>) -> Self {
        Self {
            db: Some(db),
            request_id: request_id.into(),
        }
    }

    /// Leave the reservation row in place. [`Database::record_usage`] settles it.
    pub(crate) fn disarm(&mut self) {
        self.db.take();
    }

    /// Remove the hold. A missing row is a successful no-op.
    pub(crate) async fn release(mut self) {
        self.release_now().await;
    }

    async fn release_now(&mut self) {
        let Some(db) = self.db.take() else {
            return;
        };
        let request_id = self.request_id.clone();
        let _ = db
            .run_blocking(move |db| db.release_budget(&request_id))
            .await;
    }
}

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        let Some(db) = self.db.take() else {
            return;
        };
        let request_id = std::mem::take(&mut self.request_id);
        if request_id.is_empty() {
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = db
                    .run_blocking(move |db| db.release_budget(&request_id))
                    .await;
            });
        }
    }
}

/// USD hold for one request.
///
/// Prompt tokens are `prompt_chars / 4`. Completion tokens are included only when
/// the client set `max_completion_tokens`. A model with no rate returns `0.0`.
#[must_use]
pub fn estimate_hold_usd(
    model: &str,
    prompt_chars: usize,
    max_completion_tokens: Option<u32>,
) -> f64 {
    let prompt_tokens = u32::try_from(prompt_chars / 4).unwrap_or(u32::MAX);
    let completion_tokens = max_completion_tokens.unwrap_or(0);
    let usage = proto::Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens.saturating_add(completion_tokens),
        cached_tokens: None,
        reasoning_tokens: None,
        cost_usd: None,
    };
    let cost = estimate_model_call_cost_usd(model, &usage);
    if cost.is_finite() && cost > 0.0 {
        cost
    } else {
        0.0
    }
}

/// Character count of message text used for the prompt side of a hold.
#[must_use]
pub fn message_prompt_chars(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            message
                .content
                .as_ref()
                .map(|content| content.text_for_summary().len())
                .unwrap_or(0)
        })
        .sum()
}
