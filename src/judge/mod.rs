//! Judge config and mapped verdicts over TypeSafe System One.
//!
//! A [`JudgeConfig`] names the questions and the values an application receives.
//! Build one with [`JudgeBuilder`] or load JSON with [`JudgeConfig::from_json`].
//! [`run`] calls System One, then applies the output rules.

mod builder;
mod config;
mod run;

use thiserror::Error;

pub use builder::JudgeBuilder;
pub use config::{
    JudgeConfig, MappedValue, OnError, OutputSpec, QuestionSpec, RepeatOutput, RepeatSpec,
    UnknownPolicy, configs_from_json,
};
pub use run::{JudgeInput, JudgeItem, JudgeOverrides, Verdict, VerdictValue, run};

/// A judge failure. The text is a short kind. It does not include state or an upstream body.
#[derive(Debug, Error)]
pub enum JudgeError {
    #[error("judge config is invalid: {0}")]
    Config(&'static str),
    #[error("judge request timed out")]
    Timeout,
    #[error("judge request failed")]
    Upstream,
    #[error("judge credentials are missing")]
    Credentials,
    #[error("judge model is unsupported")]
    UnsupportedModel,
    #[error("judge output is missing: {0}")]
    Missing(String),
    #[error("judge output is unknown: {0}")]
    Unknown(String),
}
