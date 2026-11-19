//! OpenAI Decisions proxy.
//!
//! Public path: `POST /v1/decisions`.
//!
//! Who can reach this: a caller who holds a gateway key.
//! Worst input: a large `input` and up to 400 questions.
//! What a failure leaks: the handler does not log `input` or answers.
//! Errors do not include the OpenAI key.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::decisions::{
    DecisionError, DecisionRequest, DecisionResponse, Input, Question, decisions, qualify_model,
};
use crate::gateway::GatewayState;
use crate::gateway::auth::{Auth, resolve_user_id};
use crate::gateway::budget::estimate_hold_usd;
use crate::gateway::error::GatewayError;
use crate::gateway::proxy::{preflight_async, record_usage_async};

#[derive(Debug, Deserialize)]
pub struct GatewayDecisionBody {
    input: Input,
    #[serde(default)]
    model: Option<String>,
    questions: Vec<Question>,
    #[serde(default)]
    user: Option<String>,
}

/// Proxy a Decisions evaluation to OpenAI.
pub async fn evaluate(
    State(state): State<Arc<GatewayState>>,
    Auth(auth): Auth,
    Json(body): Json<GatewayDecisionBody>,
) -> Result<Json<Value>, GatewayError> {
    let model_ref = qualify_model(body.model.as_deref()).map_err(decision_to_gateway)?;
    let qualified = format!("openai:{}", model_ref.model);
    let user_id = resolve_user_id(&auth, body.user.as_deref())?;
    let request_id = Uuid::new_v4().to_string();
    let prompt_chars = serde_json::to_string(&body.input)
        .map(|text| text.len())
        .unwrap_or(0)
        .saturating_add(
            serde_json::to_string(&body.questions)
                .map(|text| text.len())
                .unwrap_or(0),
        );
    let estimate = estimate_hold_usd(&qualified, prompt_chars, None);
    let mut hold = preflight_async(
        &state.db,
        &auth,
        &user_id,
        &qualified,
        &request_id,
        estimate,
        false,
    )
    .await?;

    let request = DecisionRequest {
        input: body.input,
        model: Some(qualified.clone()),
        questions: body.questions,
    };
    let response = match decisions(&state.http, &state.credentials, request).await {
        Ok(response) => response,
        Err(error) => {
            hold.release().await;
            return Err(decision_to_gateway(error));
        }
    };

    let usage = response.usage.to_proto();
    record_usage_async(
        &state.db,
        auth.key_id.as_deref(),
        &user_id,
        &qualified,
        &usage,
        0.0,
        &request_id,
    )
    .await?;
    hold.disarm();

    let value = serde_json::to_value(DecisionResponse {
        model: response.model,
        answers: response.answers,
        usage: response.usage,
    })
    .map_err(|err| GatewayError::Internal(err.to_string()))?;
    Ok(Json(value))
}

fn decision_to_gateway(err: DecisionError) -> GatewayError {
    match err {
        DecisionError::UnsupportedProvider(provider) => {
            GatewayError::bad_request(format!("provider {provider} does not support Decisions"))
        }
        DecisionError::Validation(message) => GatewayError::bad_request(message),
        DecisionError::Credentials(err) => GatewayError::upstream(err.to_string()),
        DecisionError::Http(err) => GatewayError::upstream(err.to_string()),
        DecisionError::Parse(message) => GatewayError::upstream(message),
    }
}
