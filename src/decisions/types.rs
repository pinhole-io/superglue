//! OpenAI Decisions request and response types.
//!
//! The wire shape is `POST /v1/decisions`. A question is a predicate, a choice,
//! or a score. This is the OpenAI counterpart of TypeSafe System One.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::proto;
use crate::usage::{UsageBreakdown, usage_from_breakdown};

/// Model id the Decisions API accepts.
pub const DEFAULT_MODEL: &str = "gpt-6-luna";

/// Maximum questions in one request. The API rejects a longer list.
pub const MAX_QUESTIONS: usize = 400;

/// Ordered levels on one score question. The API requires this range.
pub const SCORE_LEVELS: std::ops::RangeInclusive<usize> = 2..=10;

/// Shared evidence for every question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    /// Plain text.
    Text(String),
    /// A message list or other JSON value.
    Json(Value),
}

impl From<&str> for Input {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for Input {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Value> for Input {
    fn from(value: Value) -> Self {
        match value {
            Value::String(text) => Self::Text(text),
            other => Self::Json(other),
        }
    }
}

/// One allowed answer for a choice question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceOption {
    /// Value the model must return.
    pub value: String,
    /// When this value applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ChoiceOption {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// One level of a score rubric, lowest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreLevel {
    /// Short name of the level.
    pub label: String,
    /// What this level means.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ScoreLevel {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            description: None,
        }
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// A typed Decisions question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Probability that a statement is true.
    Predicate {
        /// Answer id.
        name: String,
        /// What to judge.
        instructions: String,
    },
    /// One value from a closed set.
    Choice {
        /// Answer id.
        name: String,
        /// What to judge.
        instructions: String,
        /// Allowed values.
        choices: Vec<ChoiceOption>,
    },
    /// A rubric score.
    Score {
        /// Answer id.
        name: String,
        /// What to judge.
        instructions: String,
        /// Levels from lowest to highest.
        levels: Vec<ScoreLevel>,
    },
}

impl Question {
    #[must_use]
    pub fn predicate(name: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self::Predicate {
            name: name.into(),
            instructions: instructions.into(),
        }
    }

    #[must_use]
    pub fn choice(
        name: impl Into<String>,
        instructions: impl Into<String>,
        choices: Vec<ChoiceOption>,
    ) -> Self {
        Self::Choice {
            name: name.into(),
            instructions: instructions.into(),
            choices,
        }
    }

    #[must_use]
    pub fn score(
        name: impl Into<String>,
        instructions: impl Into<String>,
        levels: Vec<ScoreLevel>,
    ) -> Self {
        Self::Score {
            name: name.into(),
            instructions: instructions.into(),
            levels,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Predicate { name, .. } | Self::Choice { name, .. } | Self::Score { name, .. } => {
                name
            }
        }
    }
}

/// A choice or probability value from the API.
///
/// A choice is a string or a boolean. A score level is its integer index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChoiceValue {
    /// A string option.
    Text(String),
    /// A boolean option.
    Flag(bool),
    /// A score level index. Level 0 is the lowest level.
    Index(i64),
}

/// One option probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probability {
    /// The option this probability belongs to.
    pub value: ChoiceValue,
    /// Probability mass for `value`.
    pub probability: f64,
    /// Score-level name. Choice probabilities omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A typed Decisions answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// Predicate result.
    Predicate {
        /// Question name.
        name: Option<String>,
        /// Probability the statement is true.
        probability: f64,
    },
    /// Selected option.
    Choice {
        /// Question name.
        name: Option<String>,
        /// The selected value.
        choice: ChoiceValue,
        /// Confidence in `choice`.
        #[serde(default)]
        confidence: f64,
        /// Mass over the options.
        #[serde(default)]
        probabilities: Vec<Probability>,
    },
    /// Rubric score.
    Score {
        /// Question name.
        name: Option<String>,
        /// Probability-weighted level index.
        score: f64,
        /// Confidence in the score.
        #[serde(default)]
        confidence: f64,
        /// Mass over the levels.
        #[serde(default)]
        probabilities: Vec<Probability>,
    },
    /// The model declined this question.
    Refusal {
        /// Question name.
        name: Option<String>,
    },
}

impl Answer {
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Predicate { name, .. }
            | Self::Choice { name, .. }
            | Self::Score { name, .. }
            | Self::Refusal { name } => name.as_deref(),
        }
    }

    #[must_use]
    pub fn probability(&self) -> Option<f64> {
        match self {
            Self::Predicate { probability, .. } => Some(*probability),
            _ => None,
        }
    }

    #[must_use]
    pub fn choice_text(&self) -> Option<&str> {
        match self {
            Self::Choice {
                choice: ChoiceValue::Text(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        }
    }

    #[must_use]
    pub fn score(&self) -> Option<f64> {
        match self {
            Self::Score { score, .. } => Some(*score),
            _ => None,
        }
    }

    #[must_use]
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => Some(*confidence),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_refusal(&self) -> bool {
        matches!(self, Self::Refusal { .. })
    }
}

/// Token counts from a Decisions response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DecisionUsage {
    #[serde(default)]
    pub input_tokens: Option<u32>,
    #[serde(default)]
    pub output_tokens: Option<u32>,
}

impl DecisionUsage {
    #[must_use]
    pub fn to_proto(&self) -> proto::Usage {
        usage_from_breakdown(UsageBreakdown::from_counts(
            self.input_tokens.unwrap_or(0),
            self.output_tokens.unwrap_or(0),
        ))
    }
}

/// Request body for `POST /v1/decisions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub input: Input,
    pub questions: Vec<Question>,
}

impl DecisionRequest {
    #[must_use]
    pub fn new(input: impl Into<Input>, questions: Vec<Question>) -> Self {
        Self {
            model: None,
            input: input.into(),
            questions,
        }
    }

    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// Response body from `POST /v1/decisions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub model: String,
    pub answers: Vec<Answer>,
    #[serde(default)]
    pub usage: DecisionUsage,
}

impl DecisionResponse {
    /// The first answer whose name is `name`.
    #[must_use]
    pub fn answer(&self, name: &str) -> Option<&Answer> {
        self.answers
            .iter()
            .find(|answer| answer.name() == Some(name))
    }
}
