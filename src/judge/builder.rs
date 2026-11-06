//! Rust builder for a [`JudgeConfig`](super::JudgeConfig).
//!
//! `build` calls `validate`. JSON loading stays on `from_json`.

use std::collections::BTreeMap;

use crate::systemone::NoulCriteria;

use super::JudgeError;
use super::config::{
    JudgeConfig, MappedValue, OnError, OutputSpec, QuestionSpec, RepeatOutput, RepeatSpec,
    UnknownPolicy, default_model, default_timeout_ms,
};

/// Builds one judge in code. Nested steps return here from `done`.
#[derive(Debug)]
pub struct JudgeBuilder {
    config: JudgeConfig,
}

impl JudgeBuilder {
    /// Start a judge. The model and timeout use the System One defaults.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            config: JudgeConfig {
                id: id.into(),
                model: default_model(),
                timeout_ms: default_timeout_ms(),
                state_chars: None,
                on_error: OnError::Error,
                questions: BTreeMap::new(),
                outputs: Vec::new(),
                repeat: None,
            },
        }
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    #[must_use]
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.timeout_ms = timeout_ms;
        self
    }

    /// Clip text state to `chars`. `0` uses the default snippet length.
    #[must_use]
    pub fn state_chars(mut self, chars: usize) -> Self {
        self.config.state_chars = Some(chars);
        self
    }

    #[must_use]
    pub fn on_error(mut self, on_error: OnError) -> Self {
        self.config.on_error = on_error;
        self
    }

    #[must_use]
    pub fn noul(self, id: impl Into<String>, instructions: impl Into<String>) -> NoulStep {
        NoulStep {
            parent: self,
            id: id.into(),
            instructions: instructions.into(),
            yes: None,
            no: None,
        }
    }

    #[must_use]
    pub fn choice(self, id: impl Into<String>, instructions: impl Into<String>) -> ChoiceStep {
        ChoiceStep {
            parent: self,
            id: id.into(),
            instructions: instructions.into(),
            options: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn score(self, id: impl Into<String>, instructions: impl Into<String>) -> ScoreStep {
        ScoreStep {
            parent: self,
            id: id.into(),
            instructions: instructions.into(),
            levels: Vec::new(),
        }
    }

    #[must_use]
    pub fn noul_output(self, name: impl Into<String>, from: impl Into<String>) -> NoulOutputStep {
        NoulOutputStep {
            parent: self,
            name: name.into(),
            from: from.into(),
            threshold: None,
            required: false,
        }
    }

    #[must_use]
    pub fn choice_output(
        self,
        name: impl Into<String>,
        from: impl Into<String>,
    ) -> ChoiceOutputStep {
        ChoiceOutputStep {
            parent: self,
            name: name.into(),
            from: from.into(),
            min_confidence: 0.0,
            map: BTreeMap::new(),
            omit: Vec::new(),
            on_unknown: UnknownPolicy::Error,
            required: false,
            emit_confidence: false,
        }
    }

    #[must_use]
    pub fn score_output(self, name: impl Into<String>, from: impl Into<String>) -> ScoreOutputStep {
        ScoreOutputStep {
            parent: self,
            name: name.into(),
            from: from.into(),
            min: None,
            required: false,
        }
    }

    /// One Noul question, stamped once per item.
    #[must_use]
    pub fn repeat_noul(
        self,
        name: impl Into<String>,
        instructions: impl Into<String>,
    ) -> RepeatNoulStep {
        RepeatNoulStep {
            parent: self,
            name: name.into(),
            instructions: instructions.into(),
            yes: None,
            no: None,
            id_prefix: String::new(),
            threshold: None,
        }
    }

    /// One choice question, stamped once per item.
    #[must_use]
    pub fn repeat_choice(
        self,
        name: impl Into<String>,
        instructions: impl Into<String>,
    ) -> RepeatChoiceStep {
        RepeatChoiceStep {
            parent: self,
            name: name.into(),
            instructions: instructions.into(),
            options: BTreeMap::new(),
            id_prefix: String::new(),
            min_confidence: 0.0,
            map: BTreeMap::new(),
            omit: Vec::new(),
            on_unknown: UnknownPolicy::Error,
        }
    }

    /// One score question, stamped once per item.
    #[must_use]
    pub fn repeat_score(
        self,
        name: impl Into<String>,
        instructions: impl Into<String>,
    ) -> RepeatScoreStep {
        RepeatScoreStep {
            parent: self,
            name: name.into(),
            instructions: instructions.into(),
            levels: Vec::new(),
            id_prefix: String::new(),
            min: None,
        }
    }

    /// Validate the config and return it.
    pub fn build(self) -> Result<JudgeConfig, JudgeError> {
        self.config.validate()?;
        Ok(self.config)
    }
}

/// A fixed Noul question. [`done`](Self::done) returns the parent builder.
#[derive(Debug)]
pub struct NoulStep {
    parent: JudgeBuilder,
    id: String,
    instructions: String,
    yes: Option<String>,
    no: Option<String>,
}

impl NoulStep {
    #[must_use]
    pub fn criteria(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        self.yes = Some(yes.into());
        self.no = Some(no.into());
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.questions.insert(
            self.id,
            QuestionSpec::Noul {
                instructions: self.instructions,
                criteria: noul_criteria(self.yes, self.no),
            },
        );
        self.parent
    }
}

/// A fixed choice question. [`done`](Self::done) returns the parent builder.
#[derive(Debug)]
pub struct ChoiceStep {
    parent: JudgeBuilder,
    id: String,
    instructions: String,
    options: BTreeMap<String, String>,
}

impl ChoiceStep {
    #[must_use]
    pub fn option(mut self, key: impl Into<String>, description: impl Into<String>) -> Self {
        self.options.insert(key.into(), description.into());
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.questions.insert(
            self.id,
            QuestionSpec::Choice {
                instructions: self.instructions,
                options: self.options,
            },
        );
        self.parent
    }
}

/// A fixed score question. [`done`](Self::done) returns the parent builder.
#[derive(Debug)]
pub struct ScoreStep {
    parent: JudgeBuilder,
    id: String,
    instructions: String,
    levels: Vec<String>,
}

impl ScoreStep {
    #[must_use]
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.levels.push(level.into());
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.questions.insert(
            self.id,
            QuestionSpec::Score {
                instructions: self.instructions,
                levels: self.levels,
            },
        );
        self.parent
    }
}

/// A Noul output. A threshold makes the verdict a bool.
#[derive(Debug)]
pub struct NoulOutputStep {
    parent: JudgeBuilder,
    name: String,
    from: String,
    threshold: Option<f64>,
    required: bool,
}

impl NoulOutputStep {
    /// `noul >= threshold` becomes `true`.
    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.outputs.push(OutputSpec::Noul {
            name: self.name,
            from: self.from,
            threshold: self.threshold,
            required: self.required,
        });
        self.parent
    }
}

/// A choice output. [`done`](Self::done) returns the parent builder.
#[derive(Debug)]
pub struct ChoiceOutputStep {
    parent: JudgeBuilder,
    name: String,
    from: String,
    min_confidence: f64,
    map: BTreeMap<String, MappedValue>,
    omit: Vec<String>,
    on_unknown: UnknownPolicy,
    required: bool,
    emit_confidence: bool,
}

impl ChoiceOutputStep {
    #[must_use]
    pub fn min_confidence(mut self, min_confidence: f64) -> Self {
        self.min_confidence = min_confidence;
        self
    }

    #[must_use]
    pub fn map_bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.map.insert(key.into(), MappedValue::Bool(value));
        self
    }

    #[must_use]
    pub fn map_text(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.map.insert(key.into(), MappedValue::Text(value.into()));
        self
    }

    #[must_use]
    pub fn map_number(mut self, key: impl Into<String>, value: f64) -> Self {
        self.map.insert(key.into(), MappedValue::Number(value));
        self
    }

    #[must_use]
    pub fn omit(mut self, value: impl Into<String>) -> Self {
        self.omit.push(value.into());
        self
    }

    #[must_use]
    pub fn on_unknown(mut self, policy: UnknownPolicy) -> Self {
        self.on_unknown = policy;
        self
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Also store `{name}_confidence` as a number.
    #[must_use]
    pub fn emit_confidence(mut self) -> Self {
        self.emit_confidence = true;
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.outputs.push(OutputSpec::Choice {
            name: self.name,
            from: self.from,
            min_confidence: self.min_confidence,
            map: self.map,
            omit: self.omit,
            on_unknown: self.on_unknown,
            required: self.required,
            emit_confidence: self.emit_confidence,
        });
        self.parent
    }
}

/// A score output. Scores below `min` are omitted.
#[derive(Debug)]
pub struct ScoreOutputStep {
    parent: JudgeBuilder,
    name: String,
    from: String,
    min: Option<f64>,
    required: bool,
}

impl ScoreOutputStep {
    #[must_use]
    pub fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.outputs.push(OutputSpec::Score {
            name: self.name,
            from: self.from,
            min: self.min,
            required: self.required,
        });
        self.parent
    }
}

/// A repeated Noul. The verdict map is keyed by item id.
#[derive(Debug)]
pub struct RepeatNoulStep {
    parent: JudgeBuilder,
    name: String,
    instructions: String,
    yes: Option<String>,
    no: Option<String>,
    id_prefix: String,
    threshold: Option<f64>,
}

impl RepeatNoulStep {
    #[must_use]
    pub fn criteria(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        self.yes = Some(yes.into());
        self.no = Some(no.into());
        self
    }

    /// Prefix on the wire question id. The verdict key stays the item id.
    #[must_use]
    pub fn id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = prefix.into();
        self
    }

    /// `noul >= threshold` becomes `true`.
    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.repeat = Some(RepeatSpec {
            id_prefix: self.id_prefix,
            template: QuestionSpec::Noul {
                instructions: self.instructions,
                criteria: noul_criteria(self.yes, self.no),
            },
            output: RepeatOutput::Noul {
                name: self.name,
                threshold: self.threshold,
            },
        });
        self.parent
    }
}

/// A repeated choice. Item options replace the template options.
#[derive(Debug)]
pub struct RepeatChoiceStep {
    parent: JudgeBuilder,
    name: String,
    instructions: String,
    options: BTreeMap<String, String>,
    id_prefix: String,
    min_confidence: f64,
    map: BTreeMap<String, MappedValue>,
    omit: Vec<String>,
    on_unknown: UnknownPolicy,
}

impl RepeatChoiceStep {
    #[must_use]
    pub fn option(mut self, key: impl Into<String>, description: impl Into<String>) -> Self {
        self.options.insert(key.into(), description.into());
        self
    }

    #[must_use]
    pub fn id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = prefix.into();
        self
    }

    #[must_use]
    pub fn min_confidence(mut self, min_confidence: f64) -> Self {
        self.min_confidence = min_confidence;
        self
    }

    #[must_use]
    pub fn map_bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.map.insert(key.into(), MappedValue::Bool(value));
        self
    }

    #[must_use]
    pub fn omit(mut self, value: impl Into<String>) -> Self {
        self.omit.push(value.into());
        self
    }

    #[must_use]
    pub fn on_unknown(mut self, policy: UnknownPolicy) -> Self {
        self.on_unknown = policy;
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.repeat = Some(RepeatSpec {
            id_prefix: self.id_prefix,
            template: QuestionSpec::Choice {
                instructions: self.instructions,
                options: self.options,
            },
            output: RepeatOutput::Choice {
                name: self.name,
                min_confidence: self.min_confidence,
                map: self.map,
                omit: self.omit,
                on_unknown: self.on_unknown,
            },
        });
        self.parent
    }
}

/// A repeated score. Item levels replace the template levels.
#[derive(Debug)]
pub struct RepeatScoreStep {
    parent: JudgeBuilder,
    name: String,
    instructions: String,
    levels: Vec<String>,
    id_prefix: String,
    min: Option<f64>,
}

impl RepeatScoreStep {
    #[must_use]
    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.levels.push(level.into());
        self
    }

    #[must_use]
    pub fn id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.id_prefix = prefix.into();
        self
    }

    #[must_use]
    pub fn min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    #[must_use]
    pub fn done(mut self) -> JudgeBuilder {
        self.parent.config.repeat = Some(RepeatSpec {
            id_prefix: self.id_prefix,
            template: QuestionSpec::Score {
                instructions: self.instructions,
                levels: self.levels,
            },
            output: RepeatOutput::Score {
                name: self.name,
                min: self.min,
            },
        });
        self.parent
    }
}

fn noul_criteria(yes: Option<String>, no: Option<String>) -> Option<NoulCriteria> {
    if yes.is_none() && no.is_none() {
        None
    } else {
        Some(NoulCriteria { yes, no })
    }
}
