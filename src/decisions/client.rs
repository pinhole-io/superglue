//! HTTP client for the OpenAI Decisions API.

use std::time::Instant;

use reqwest::StatusCode;
use secrecy::ExposeSecret;
use serde_json::Value;
use thiserror::Error;
use tracing::instrument;

use super::types::{
    DEFAULT_MODEL, DecisionRequest, DecisionResponse, Input, MAX_QUESTIONS, Question, SCORE_LEVELS,
};
use crate::http::{Error as HttpError, HttpClient, join_base_url};
use crate::providers::{
    CredentialsError, ProviderCredentials, ProviderId, parse_model_ref, rate_limit_key_for,
};

/// Path on both OpenAI and the SuperGlue gateway.
pub const DECISIONS_PATH: &str = "/v1/decisions";

/// Errors from a Decisions call.
#[derive(Debug, Error)]
pub enum DecisionError {
    #[error(transparent)]
    Credentials(#[from] CredentialsError),
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("decisions request failed validation: {0}")]
    Validation(String),
    #[error("failed to parse decisions response: {0}")]
    Parse(String),
    #[error("unsupported provider for decisions: {0}")]
    UnsupportedProvider(ProviderId),
}

/// Parse a model string into an OpenAI Decisions model ref.
///
/// A bare id becomes `openai:<id>`. The only supported provider is OpenAI.
pub fn qualify_model(raw: Option<&str>) -> Result<crate::providers::ModelRef, DecisionError> {
    let raw = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL);
    let model_ref = if raw.contains(':') {
        parse_model_ref(raw)
    } else {
        parse_model_ref(&format!("openai:{raw}"))
    };
    if !model_ref.provider.uses_decisions() {
        return Err(DecisionError::UnsupportedProvider(model_ref.provider));
    }
    Ok(model_ref)
}

/// Model id sent on the wire.
///
/// A direct OpenAI call uses the bare id. Any other base URL keeps `openai:`.
#[must_use]
pub fn wire_model(model: &str, base_url: &str) -> String {
    let Ok(model_ref) = qualify_model(Some(model)) else {
        return model.to_string();
    };
    if base_url == ProviderId::OpenAi.default_base_url() {
        model_ref.model
    } else {
        format!("openai:{}", model_ref.model)
    }
}

/// Build a request from JSON values (language bindings).
pub fn request_from_parts(
    input: Value,
    questions: Value,
    model: Option<String>,
) -> Result<DecisionRequest, DecisionError> {
    let questions: Vec<Question> =
        serde_json::from_value(questions).map_err(|err| DecisionError::Parse(err.to_string()))?;
    Ok(DecisionRequest {
        input: Input::from(input),
        model: model.filter(|value| !value.trim().is_empty()),
        questions,
    })
}

/// Evaluate `request` with the OpenAI Decisions API.
///
/// Do not log `input` or answer values. Those can carry untrusted payload text.
#[instrument(
    skip(http, credentials, request),
    fields(model = tracing::field::Empty, request_id = tracing::field::Empty)
)]
pub async fn decisions(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    request: DecisionRequest,
) -> Result<DecisionResponse, DecisionError> {
    let start = Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();
    let model = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string();
    let questions = request.questions.len();
    tracing::Span::current().record("model", tracing::field::display(&model));
    tracing::Span::current().record("request_id", tracing::field::display(&request_id));
    crate::telemetry::openinference::tag_llm(&request_id, &model, "openai");
    metrics::counter!(crate::telemetry::metrics::DECISIONS_TOTAL, "model" => model.clone())
        .increment(1);
    tracing::info!(
        request_id = %request_id,
        model = %model,
        questions,
        "decisions started"
    );

    match evaluate(http, credentials, request).await {
        Ok(response) => {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            metrics::histogram!(
                crate::telemetry::metrics::DECISIONS_DURATION_MS,
                "model" => response.model.clone()
            )
            .record(elapsed_ms);
            tracing::info!(
                request_id = %request_id,
                model = %response.model,
                questions = response.answers.len(),
                elapsed_ms,
                "decisions finished"
            );
            crate::telemetry::openinference::set_usage(&response.usage.to_proto());
            Ok(response)
        }
        Err(err) => {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            let error_kind = decision_error_kind(&err);
            metrics::counter!(
                crate::telemetry::metrics::DECISIONS_ERRORS,
                "model" => model.clone(),
                "error_kind" => error_kind
            )
            .increment(1);
            tracing::warn!(
                request_id = %request_id,
                model = %model,
                questions,
                elapsed_ms,
                error_kind,
                error = %err,
                "decisions failed"
            );
            crate::telemetry::openinference::fail_current(&err.to_string());
            Err(err)
        }
    }
}

async fn evaluate(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    mut request: DecisionRequest,
) -> Result<DecisionResponse, DecisionError> {
    let model_ref = qualify_model(request.model.as_deref())?;
    credentials.key_for(model_ref.provider)?;
    validate(&request)?;
    let base_url = credentials.base_url_for(model_ref.provider);
    let url = join_base_url(&base_url, DECISIONS_PATH);
    let key = credentials.key_for(model_ref.provider)?;
    let rate_limit_key = rate_limit_key_for(&model_ref, credentials)?;
    request.model = Some(wire_model(
        request.model.as_deref().unwrap_or(DEFAULT_MODEL),
        &base_url,
    ));
    let payload =
        serde_json::to_value(&request).map_err(|err| DecisionError::Parse(err.to_string()))?;
    let auth = format!("Bearer {}", key.expose_secret());
    let value = http
        .post_json_with_headers(
            &url,
            &payload,
            &[("Authorization", auth.as_str())],
            Some(rate_limit_key),
        )
        .await
        .map_err(map_http_error)?;
    serde_json::from_value(value).map_err(|err| DecisionError::Parse(err.to_string()))
}

fn validate(request: &DecisionRequest) -> Result<(), DecisionError> {
    match &request.input {
        Input::Text(text) if text.trim().is_empty() => {
            return Err(DecisionError::Validation("input text is empty".to_string()));
        }
        Input::Json(Value::Null) => {
            return Err(DecisionError::Validation("input is null".to_string()));
        }
        Input::Text(_) | Input::Json(_) => {}
    }
    if request.questions.is_empty() {
        return Err(DecisionError::Validation(
            "a decision needs at least one question".to_string(),
        ));
    }
    if request.questions.len() > MAX_QUESTIONS {
        return Err(DecisionError::Validation(format!(
            "a decision accepts at most {MAX_QUESTIONS} questions"
        )));
    }
    let mut seen = Vec::with_capacity(request.questions.len());
    for question in &request.questions {
        let name = question.name().trim();
        if name.is_empty() {
            return Err(DecisionError::Validation(
                "question name is empty".to_string(),
            ));
        }
        if seen.iter().any(|existing: &String| existing == name) {
            return Err(DecisionError::Validation(format!(
                "duplicate question name {name}"
            )));
        }
        seen.push(name.to_string());
        match question {
            Question::Predicate { instructions, .. } if instructions.trim().is_empty() => {
                return Err(DecisionError::Validation(format!(
                    "question {name} has empty instructions"
                )));
            }
            Question::Choice {
                instructions,
                choices,
                ..
            } => {
                if instructions.trim().is_empty() {
                    return Err(DecisionError::Validation(format!(
                        "question {name} has empty instructions"
                    )));
                }
                if choices.is_empty() {
                    return Err(DecisionError::Validation(format!(
                        "question {name} has no choices"
                    )));
                }
            }
            Question::Score {
                instructions,
                levels,
                ..
            } => {
                if instructions.trim().is_empty() {
                    return Err(DecisionError::Validation(format!(
                        "question {name} has empty instructions"
                    )));
                }
                if !SCORE_LEVELS.contains(&levels.len()) {
                    return Err(DecisionError::Validation(format!(
                        "question {name} needs 2 to 10 score levels"
                    )));
                }
            }
            Question::Predicate { .. } => {}
        }
    }
    Ok(())
}

fn decision_error_kind(err: &DecisionError) -> &'static str {
    match err {
        DecisionError::Credentials(_) => "credentials",
        DecisionError::Http(_) => "http",
        DecisionError::Validation(_) => "validation",
        DecisionError::Parse(_) => "parse",
        DecisionError::UnsupportedProvider(_) => "unsupported_provider",
    }
}

fn map_http_error(err: HttpError) -> DecisionError {
    if err.status() == Some(StatusCode::UNPROCESSABLE_ENTITY) {
        let message = err.provider_message().unwrap_or_else(|| err.to_string());
        return DecisionError::Validation(message);
    }
    DecisionError::Http(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualify_defaults_to_gpt_6_luna() {
        let model_ref = qualify_model(None).expect("default");
        assert_eq!(model_ref.provider, ProviderId::OpenAi);
        assert_eq!(model_ref.model, DEFAULT_MODEL);
    }

    #[test]
    fn qualify_rejects_typesafe() {
        let err = qualify_model(Some("typesafe:jev-latest")).expect_err("typesafe");
        assert!(matches!(err, DecisionError::UnsupportedProvider(_)));
    }

    #[test]
    fn wire_model_strips_the_prefix_for_openai() {
        assert_eq!(
            wire_model(DEFAULT_MODEL, ProviderId::OpenAi.default_base_url()),
            DEFAULT_MODEL
        );
        assert_eq!(
            wire_model(DEFAULT_MODEL, "http://127.0.0.1:9"),
            format!("openai:{DEFAULT_MODEL}")
        );
    }
}
