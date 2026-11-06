//! Run a judge config and map the System One answers.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::HttpClient;
use crate::providers::ProviderCredentials;
use crate::systemone::{
    Answer, State, SystemOneError, SystemOneRequest, SystemOneResponse, clip_state, system_one,
};

use super::config::{
    MappedValue, OnError, OutputSpec, QuestionSpec, RepeatOutput, RepeatSpec, UnknownPolicy,
};
use super::{JudgeConfig, JudgeError};

/// One repeated question. `options` fill a choice template. `levels` fill a score template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeItem {
    pub id: String,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub levels: Vec<String>,
}

impl JudgeItem {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            options: BTreeMap::new(),
            levels: Vec::new(),
        }
    }

    #[must_use]
    pub fn option(mut self, key: impl Into<String>, description: impl Into<String>) -> Self {
        self.options.insert(key.into(), description.into());
        self
    }

    #[must_use]
    pub fn levels(mut self, levels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.levels = levels.into_iter().map(Into::into).collect();
        self
    }
}

/// State plus the items for a repeat template.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeInput {
    pub state: State,
    pub items: Vec<JudgeItem>,
}

impl JudgeInput {
    #[must_use]
    pub fn new(state: impl Into<State>) -> Self {
        Self {
            state: state.into(),
            items: Vec::new(),
        }
    }

    #[must_use]
    pub fn text(state: impl Into<String>) -> Self {
        Self::new(State::Text(state.into()))
    }

    #[must_use]
    pub fn json(state: Value) -> Self {
        Self::new(State::Json(state))
    }

    #[must_use]
    pub fn with_items(mut self, items: Vec<JudgeItem>) -> Self {
        self.items = items;
        self
    }
}

/// Optional model and timeout that replace the config values for one run.
#[derive(Debug, Clone, Default)]
pub struct JudgeOverrides {
    pub model: Option<String>,
    pub timeout: Option<Duration>,
}

/// Mapped values for one judge run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Verdict {
    pub values: BTreeMap<String, VerdictValue>,
}

impl Verdict {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&VerdictValue> {
        self.values.get(name)
    }

    #[must_use]
    pub fn bool(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(VerdictValue::as_bool)
    }

    #[must_use]
    pub fn number(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(VerdictValue::as_number)
    }

    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(VerdictValue::as_text)
    }

    #[must_use]
    pub fn map(&self, name: &str) -> Option<&BTreeMap<String, VerdictValue>> {
        self.get(name).and_then(VerdictValue::as_map)
    }
}

/// One mapped output. A repeat output is a map keyed by item id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VerdictValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Map(BTreeMap<String, VerdictValue>),
}

impl VerdictValue {
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_map(&self) -> Option<&BTreeMap<String, VerdictValue>> {
        match self {
            Self::Map(value) => Some(value),
            _ => None,
        }
    }
}

/// Evaluate `config` against `input`.
///
/// Do not log `state`, instructions, or verdict values.
pub async fn run(
    http: &HttpClient,
    credentials: &ProviderCredentials,
    config: &JudgeConfig,
    input: JudgeInput,
    overrides: JudgeOverrides,
) -> Result<Verdict, JudgeError> {
    config.validate()?;
    let JudgeInput { state, items } = input;
    if config.questions.is_empty() && items.is_empty() {
        return Ok(Verdict::empty());
    }

    let questions = build_questions(config, &items);
    if questions.0.is_empty() {
        return Ok(Verdict::empty());
    }

    let model = overrides
        .model
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| config.model.clone());
    let timeout = overrides
        .timeout
        .unwrap_or_else(|| Duration::from_millis(config.timeout_ms));
    let request =
        SystemOneRequest::new(clip_input_state(config, state), questions).with_model(model);

    tracing::info!(
        judge_id = %config.id,
        questions = request.questions.len(),
        "judge started"
    );
    let work = system_one(http, credentials, request);
    let response = match tokio::time::timeout(timeout, work).await {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => return finish_error(config, map_system_one_error(err)),
        Err(_) => return finish_error(config, JudgeError::Timeout),
    };
    match apply_response(config, &items, &response) {
        Ok(verdict) => {
            tracing::info!(judge_id = %config.id, "judge finished");
            Ok(verdict)
        }
        Err(err) => finish_error(config, err),
    }
}

fn finish_error(config: &JudgeConfig, err: JudgeError) -> Result<Verdict, JudgeError> {
    tracing::warn!(
        judge_id = %config.id,
        kind = error_kind(&err),
        "judge failed"
    );
    match config.on_error {
        OnError::Empty => Ok(Verdict::empty()),
        OnError::Error => Err(err),
    }
}

fn error_kind(err: &JudgeError) -> &'static str {
    match err {
        JudgeError::Config(_) => "config",
        JudgeError::Timeout => "timeout",
        JudgeError::Upstream => "upstream",
        JudgeError::Credentials => "credentials",
        JudgeError::UnsupportedModel => "model",
        JudgeError::Missing(_) => "missing",
        JudgeError::Unknown(_) => "unknown",
    }
}

fn map_system_one_error(err: SystemOneError) -> JudgeError {
    match err {
        SystemOneError::Credentials(_) => JudgeError::Credentials,
        SystemOneError::UnsupportedProvider(_) => JudgeError::UnsupportedModel,
        SystemOneError::Http(_) | SystemOneError::Validation(_) | SystemOneError::Parse(_) => {
            JudgeError::Upstream
        }
    }
}

fn clip_input_state(config: &JudgeConfig, state: State) -> State {
    let Some(max) = config.state_chars else {
        return state;
    };
    match state {
        State::Text(text) => State::Text(clip_state(&text, max)),
        other => other,
    }
}

fn build_questions(config: &JudgeConfig, items: &[JudgeItem]) -> crate::systemone::Questions {
    let mut questions = crate::systemone::Questions::new();
    for (id, spec) in &config.questions {
        questions.insert(id.clone(), spec.to_question(None, None));
    }
    if let Some(repeat) = &config.repeat {
        for item in items {
            let wire_id = format!("{}{}", repeat.id_prefix, item.id);
            let options = if item.options.is_empty() {
                None
            } else {
                Some(&item.options)
            };
            let levels = if item.levels.is_empty() {
                None
            } else {
                Some(item.levels.as_slice())
            };
            questions.insert(wire_id, repeat.template.to_question(options, levels));
        }
    }
    questions
}

fn apply_response(
    config: &JudgeConfig,
    items: &[JudgeItem],
    response: &SystemOneResponse,
) -> Result<Verdict, JudgeError> {
    let mut values = BTreeMap::new();
    for output in &config.outputs {
        let options = config
            .questions
            .get(output.from())
            .map(QuestionSpec::option_keys)
            .unwrap_or_default();
        match map_output(
            output.name(),
            output,
            response.answer(output.from()),
            &options,
        )? {
            Some(value) => {
                values.insert(output.name().to_string(), value);
                record_choice_confidence(&mut values, output, response);
            }
            None if output.required() => {
                return Err(JudgeError::Missing(output.name().to_string()));
            }
            None => {}
        }
    }
    if let Some(repeat) = &config.repeat {
        let mut mapped = BTreeMap::new();
        for item in items {
            let wire_id = format!("{}{}", repeat.id_prefix, item.id);
            let options = repeat_option_keys(repeat, item);
            if let Some(value) = map_repeat(
                repeat.output.name(),
                &repeat.output,
                response.answer(&wire_id),
                &options,
            )? {
                mapped.insert(item.id.clone(), value);
            }
        }
        if !mapped.is_empty() {
            values.insert(repeat.output.name().to_string(), VerdictValue::Map(mapped));
        }
    }
    Ok(Verdict { values })
}

fn repeat_option_keys(repeat: &RepeatSpec, item: &JudgeItem) -> BTreeMap<String, String> {
    if !item.options.is_empty() {
        return item.options.clone();
    }
    repeat.template.option_keys()
}

fn map_output(
    name: &str,
    output: &OutputSpec,
    answer: Option<&Answer>,
    options: &BTreeMap<String, String>,
) -> Result<Option<VerdictValue>, JudgeError> {
    match output {
        OutputSpec::Noul { threshold, .. } => map_noul(answer, *threshold),
        OutputSpec::Choice {
            min_confidence,
            map,
            omit,
            on_unknown,
            ..
        } => map_choice(
            name,
            answer,
            *min_confidence,
            map,
            omit,
            *on_unknown,
            options,
        ),
        OutputSpec::Score { min, .. } => map_score(answer, *min),
    }
}

fn map_repeat(
    name: &str,
    output: &RepeatOutput,
    answer: Option<&Answer>,
    options: &BTreeMap<String, String>,
) -> Result<Option<VerdictValue>, JudgeError> {
    match output {
        RepeatOutput::Noul { threshold, .. } => map_noul(answer, *threshold),
        RepeatOutput::Choice {
            min_confidence,
            map,
            omit,
            on_unknown,
            ..
        } => map_choice(
            name,
            answer,
            *min_confidence,
            map,
            omit,
            *on_unknown,
            options,
        ),
        RepeatOutput::Score { min, .. } => map_score(answer, *min),
    }
}

fn map_noul(
    answer: Option<&Answer>,
    threshold: Option<f64>,
) -> Result<Option<VerdictValue>, JudgeError> {
    let Some(noul) = answer.and_then(Answer::noul) else {
        return Ok(None);
    };
    Ok(Some(match threshold {
        Some(threshold) => VerdictValue::Bool(noul >= threshold),
        None => VerdictValue::Number(noul),
    }))
}

fn map_score(
    answer: Option<&Answer>,
    min: Option<f64>,
) -> Result<Option<VerdictValue>, JudgeError> {
    let Some(score) = answer.and_then(Answer::score) else {
        return Ok(None);
    };
    if min.is_some_and(|min| score < min) {
        return Ok(None);
    }
    Ok(Some(VerdictValue::Number(score)))
}

fn map_choice(
    name: &str,
    answer: Option<&Answer>,
    min_confidence: f64,
    map: &BTreeMap<String, MappedValue>,
    omit: &[String],
    on_unknown: UnknownPolicy,
    options: &BTreeMap<String, String>,
) -> Result<Option<VerdictValue>, JudgeError> {
    let Some(Answer::Choice {
        choice, confidence, ..
    }) = answer
    else {
        return Ok(None);
    };
    if *confidence < min_confidence {
        return Ok(None);
    }
    if omit.iter().any(|item| item == choice) {
        return Ok(None);
    }
    if !map.is_empty() {
        return match map.get(choice) {
            Some(value) => Ok(Some(mapped_to_value(value))),
            None => unknown(name, choice, on_unknown),
        };
    }
    if !options.is_empty() && !options.contains_key(choice) {
        return unknown(name, choice, on_unknown);
    }
    Ok(Some(VerdictValue::Text(choice.clone())))
}

fn record_choice_confidence(
    values: &mut BTreeMap<String, VerdictValue>,
    output: &OutputSpec,
    response: &SystemOneResponse,
) {
    let OutputSpec::Choice {
        emit_confidence: true,
        name,
        from,
        ..
    } = output
    else {
        return;
    };
    let key = format!("{name}_confidence");
    if values.contains_key(&key) {
        return;
    }
    let Some(confidence) = response.answer(from).and_then(Answer::confidence) else {
        return;
    };
    values.insert(key, VerdictValue::Number(confidence));
}

fn unknown(
    name: &str,
    choice: &str,
    policy: UnknownPolicy,
) -> Result<Option<VerdictValue>, JudgeError> {
    match policy {
        UnknownPolicy::Error => Err(JudgeError::Unknown(name.to_string())),
        UnknownPolicy::Omit => Ok(None),
        UnknownPolicy::Pass => Ok(Some(VerdictValue::Text(choice.to_string()))),
    }
}

fn mapped_to_value(value: &MappedValue) -> VerdictValue {
    match value {
        MappedValue::Bool(value) => VerdictValue::Bool(*value),
        MappedValue::Number(value) => VerdictValue::Number(*value),
        MappedValue::Text(value) => VerdictValue::Text(value.clone()),
    }
}
