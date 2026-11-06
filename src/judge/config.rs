//! Serde judge config. Load it with [`JudgeConfig::from_json`] or build it in Rust.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::JudgeError;
use crate::systemone::{DEFAULT_MODEL, NoulCriteria, Question};

/// What the runner returns when the upstream call fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnError {
    /// Return [`JudgeError`](super::JudgeError).
    #[default]
    Error,
    /// Return an empty verdict.
    Empty,
}

/// What to do when a choice is outside the map and the option list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UnknownPolicy {
    /// Fail the call.
    #[default]
    Error,
    /// Drop the output.
    Omit,
    /// Return the raw choice so the caller can apply its own check.
    Pass,
}

/// A value an application receives for one choice option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MappedValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// One fixed System One question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum QuestionSpec {
    Noul {
        instructions: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        options: BTreeMap<String, String>,
    },
    Score {
        instructions: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        levels: Vec<String>,
    },
}

impl QuestionSpec {
    pub(crate) fn to_question(
        &self,
        item_options: Option<&BTreeMap<String, String>>,
        item_levels: Option<&[String]>,
    ) -> Question {
        match self {
            Self::Noul {
                instructions,
                criteria,
            } => Question::Noul {
                instructions: instructions.clone(),
                criteria: criteria.clone(),
            },
            Self::Choice {
                instructions,
                options,
            } => {
                let options = item_options
                    .filter(|options| !options.is_empty())
                    .unwrap_or(options);
                Question::Choice {
                    instructions: instructions.clone(),
                    criteria: options
                        .iter()
                        .map(|(key, description)| (key.clone(), Value::String(description.clone())))
                        .collect(),
                }
            }
            Self::Score {
                instructions,
                levels,
            } => {
                let levels = item_levels
                    .filter(|levels| !levels.is_empty())
                    .unwrap_or(levels.as_slice());
                Question::Score {
                    instructions: instructions.clone(),
                    criteria: levels.to_vec(),
                }
            }
        }
    }

    pub(crate) fn option_keys(&self) -> BTreeMap<String, String> {
        match self {
            Self::Choice { options, .. } => options.clone(),
            Self::Noul { .. } | Self::Score { .. } => BTreeMap::new(),
        }
    }
}

/// How one fixed answer becomes a verdict value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum OutputSpec {
    Noul {
        name: String,
        from: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        threshold: Option<f64>,
        #[serde(default)]
        required: bool,
    },
    Choice {
        name: String,
        from: String,
        #[serde(default)]
        min_confidence: f64,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        map: BTreeMap<String, MappedValue>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        omit: Vec<String>,
        #[serde(default)]
        on_unknown: UnknownPolicy,
        #[serde(default)]
        required: bool,
        /// Also store `{name}_confidence` as a number.
        #[serde(default, skip_serializing_if = "is_false")]
        emit_confidence: bool,
    },
    Score {
        name: String,
        from: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default)]
        required: bool,
    },
}

impl OutputSpec {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Noul { name, .. } | Self::Choice { name, .. } | Self::Score { name, .. } => name,
        }
    }

    pub(crate) fn from(&self) -> &str {
        match self {
            Self::Noul { from, .. } | Self::Choice { from, .. } | Self::Score { from, .. } => from,
        }
    }

    pub(crate) fn required(&self) -> bool {
        match self {
            Self::Noul { required, .. }
            | Self::Choice { required, .. }
            | Self::Score { required, .. } => *required,
        }
    }
}

/// One question stamped once per [`JudgeItem`](super::JudgeItem).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepeatSpec {
    /// Prefix on the wire question id. The verdict map key stays the item id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id_prefix: String,
    pub template: QuestionSpec,
    pub output: RepeatOutput,
}

/// How each repeated answer becomes one map entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RepeatOutput {
    Noul {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        threshold: Option<f64>,
    },
    Choice {
        name: String,
        #[serde(default)]
        min_confidence: f64,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        map: BTreeMap<String, MappedValue>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        omit: Vec<String>,
        #[serde(default)]
        on_unknown: UnknownPolicy,
    },
    Score {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
    },
}

impl RepeatOutput {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Noul { name, .. } | Self::Choice { name, .. } | Self::Score { name, .. } => name,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) fn default_model() -> String {
    format!("typesafe:{DEFAULT_MODEL}")
}

pub(super) fn default_timeout_ms() -> u64 {
    400
}

/// Everything an application sets before it runs a judge.
///
/// `state_chars` clips text state. `0` uses the default snippet length.
/// JSON state is sent as the caller built it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeConfig {
    pub id: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_chars: Option<usize>,
    #[serde(default)]
    pub on_error: OnError,
    #[serde(default)]
    pub questions: BTreeMap<String, QuestionSpec>,
    #[serde(default)]
    pub outputs: Vec<OutputSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<RepeatSpec>,
}

impl JudgeConfig {
    /// Parse one judge and reject a dangling output source.
    pub fn from_json(text: &str) -> Result<Self, JudgeError> {
        let config: Self = serde_json::from_str(text).map_err(|_| JudgeError::Config("json"))?;
        config.validate()?;
        Ok(config)
    }

    /// Reject a config whose output source does not name a question.
    pub fn validate(&self) -> Result<(), JudgeError> {
        if self.id.trim().is_empty() {
            return Err(JudgeError::Config("id is empty"));
        }
        if self.questions.is_empty() && self.repeat.is_none() {
            return Err(JudgeError::Config("questions are empty"));
        }
        let mut names = BTreeSet::new();
        for output in &self.outputs {
            if output.name().is_empty() {
                return Err(JudgeError::Config("output name is empty"));
            }
            if !names.insert(output.name().to_string()) {
                return Err(JudgeError::Config("output name is duplicated"));
            }
            if !self.questions.contains_key(output.from()) {
                return Err(JudgeError::Config("output source is missing"));
            }
        }
        if let Some(repeat) = &self.repeat {
            if repeat.output.name().is_empty() {
                return Err(JudgeError::Config("output name is empty"));
            }
            if !names.insert(repeat.output.name().to_string()) {
                return Err(JudgeError::Config("output name is duplicated"));
            }
        }
        Ok(())
    }
}

/// Parse a map of named judge configs.
pub fn configs_from_json(text: &str) -> Result<BTreeMap<String, JudgeConfig>, JudgeError> {
    let map: BTreeMap<String, JudgeConfig> =
        serde_json::from_str(text).map_err(|_| JudgeError::Config("json"))?;
    for config in map.values() {
        config.validate()?;
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_output_for_unknown_question() {
        let err = JudgeConfig::from_json(
            r#"{
                "id": "sample",
                "questions": {
                    "gate": { "type": "noul", "instructions": "Urgent?" }
                },
                "outputs": [{ "kind": "noul", "name": "urgent", "from": "missing" }]
            }"#,
        )
        .expect_err("source");
        assert!(matches!(
            err,
            JudgeError::Config("output source is missing")
        ));
    }

    #[test]
    fn parses_choice_map() {
        let config = JudgeConfig::from_json(
            r#"{
                "id": "sample",
                "questions": {
                    "gate": {
                        "type": "choice",
                        "instructions": "Which gate?",
                        "options": { "brief": "Plan first.", "direct": "Answer now." }
                    }
                },
                "outputs": [{
                    "kind": "choice",
                    "name": "expand",
                    "from": "gate",
                    "map": { "brief": true, "direct": false },
                    "required": true
                }]
            }"#,
        )
        .expect("config");
        assert_eq!(config.model, "typesafe:jev-latest");
        assert_eq!(config.timeout_ms, 400);
        assert!(matches!(
            &config.outputs[0],
            OutputSpec::Choice { required: true, .. }
        ));
    }

    #[test]
    fn builder_makes_a_choice_and_rejects_a_dangling_output() {
        use crate::judge::JudgeBuilder;

        let config = JudgeBuilder::new("sample")
            .choice("gate", "Which gate?")
            .option("brief", "Plan first.")
            .option("direct", "Answer now.")
            .done()
            .choice_output("expand", "gate")
            .map_bool("brief", true)
            .map_bool("direct", false)
            .required()
            .done()
            .build()
            .expect("choice judge");
        assert!(config.validate().is_ok());
        assert!(matches!(
            config.questions.get("gate"),
            Some(QuestionSpec::Choice { .. })
        ));

        let err = JudgeBuilder::new("sample")
            .choice("gate", "Which gate?")
            .done()
            .choice_output("expand", "missing")
            .done()
            .build()
            .expect_err("dangling output");
        assert!(matches!(
            err,
            JudgeError::Config("output source is missing")
        ));
    }
}
