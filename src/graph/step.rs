//! One graph step. Built-in steps call [`Client`](crate::Client).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::agents::AgentSpec;
use crate::client::{CallOptions, Client};
use crate::embeddings::EmbedError;
use crate::judge::{JudgeConfig, JudgeInput};

use super::GraphError;
use super::state::{lookup, render_template};

/// Host code for a Rust-only step. Closures do not appear in a [`super::GraphSpec`].
#[async_trait]
pub trait GraphCall: Send + Sync {
    /// Read `state` and return the JSON value this node writes.
    async fn call(&self, client: &Client, state: &Value) -> Result<Value, GraphError>;
}

pub(crate) enum Step {
    Agent {
        spec: AgentSpec,
        prompt: String,
    },
    Complete {
        prompt: String,
    },
    Judge {
        config: JudgeConfig,
        input: String,
    },
    Embed {
        model: String,
        input: String,
        dimensions: Option<usize>,
    },
    Tool {
        name: String,
        args: String,
    },
    Map {
        template: String,
    },
    Call {
        runner: Arc<dyn GraphCall>,
    },
}

impl Clone for Step {
    fn clone(&self) -> Self {
        match self {
            Self::Agent { spec, prompt } => Self::Agent {
                spec: spec.clone(),
                prompt: prompt.clone(),
            },
            Self::Complete { prompt } => Self::Complete {
                prompt: prompt.clone(),
            },
            Self::Judge { config, input } => Self::Judge {
                config: config.clone(),
                input: input.clone(),
            },
            Self::Embed {
                model,
                input,
                dimensions,
            } => Self::Embed {
                model: model.clone(),
                input: input.clone(),
                dimensions: *dimensions,
            },
            Self::Tool { name, args } => Self::Tool {
                name: name.clone(),
                args: args.clone(),
            },
            Self::Map { template } => Self::Map {
                template: template.clone(),
            },
            Self::Call { runner } => Self::Call {
                runner: Arc::clone(runner),
            },
        }
    }
}

/// A route evaluated after the source node writes its value.
pub(crate) enum Route {
    /// Walk `path` on the value this node just wrote. The first match wins.
    Equals {
        path: String,
        expected: Value,
        target: String,
    },
    /// Host predicate over the full state. Absent from JSON specs.
    Pred {
        pred: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
        target: String,
    },
}

impl Clone for Route {
    fn clone(&self) -> Self {
        match self {
            Self::Equals {
                path,
                expected,
                target,
            } => Self::Equals {
                path: path.clone(),
                expected: expected.clone(),
                target: target.clone(),
            },
            Self::Pred { pred, target } => Self::Pred {
                pred: Arc::clone(pred),
                target: target.clone(),
            },
        }
    }
}

impl Route {
    pub(crate) fn target(&self) -> &str {
        match self {
            Self::Equals { target, .. } | Self::Pred { target, .. } => target,
        }
    }

    pub(crate) fn is_closure(&self) -> bool {
        matches!(self, Self::Pred { .. })
    }

    /// `written` is this node's step result. `state` is the object after the wave's writes.
    pub(crate) fn matches(&self, written: &Value, state: &Value) -> Result<bool, GraphError> {
        match self {
            Self::Equals { path, expected, .. } => {
                let value = lookup(written, path)?;
                Ok(value == expected)
            }
            Self::Pred { pred, .. } => Ok(pred(state)),
        }
    }
}

impl Step {
    pub(crate) fn is_call(&self) -> bool {
        matches!(self, Self::Call { .. })
    }

    pub(crate) fn embed_mut(&mut self) -> Option<&mut Option<usize>> {
        match self {
            Self::Embed { dimensions, .. } => Some(dimensions),
            _ => None,
        }
    }

    pub(crate) async fn execute(
        &self,
        client: &Client,
        state: &Value,
    ) -> Result<Value, GraphError> {
        match self {
            Self::Agent { spec, prompt } => {
                let prompt = render_template(prompt, state)?;
                let outcome = client
                    .run_agent(spec.clone(), prompt, CallOptions::default())
                    .await?;
                Ok(Value::String(outcome.content.unwrap_or_default()))
            }
            Self::Complete { prompt } => {
                let prompt = render_template(prompt, state)?;
                let outcome = client.complete(prompt, CallOptions::default()).await?;
                Ok(Value::String(outcome.content.unwrap_or_default()))
            }
            Self::Judge { config, input } => {
                let value = lookup(state, input)?.clone();
                let judge_input = match value {
                    Value::String(text) => JudgeInput::text(text),
                    other => JudgeInput::json(other),
                };
                let verdict = client.judge(config, judge_input).await?;
                serde_json::to_value(verdict).map_err(|_| GraphError::BadType(input.clone()))
            }
            Self::Embed {
                model,
                input,
                dimensions,
            } => {
                let value = lookup(state, input)?;
                let Value::String(text) = value else {
                    return Err(GraphError::BadType(input.clone()));
                };
                let outcome = client
                    .embed(model.clone(), text.clone(), *dimensions)
                    .await?;
                let vector = outcome
                    .embeddings
                    .first()
                    .ok_or(EmbedError::MissingEmbedding)?;
                Ok(json!(vector.vector))
            }
            Self::Tool { name, args } => {
                let value = lookup(state, args)?.clone();
                let Value::Object(_) = &value else {
                    return Err(GraphError::BadType(args.clone()));
                };
                Ok(client.invoke_tool(name, value).await?)
            }
            Self::Map { template } => Ok(Value::String(render_template(template, state)?)),
            Self::Call { runner } => runner.call(client, state).await,
        }
    }
}

/// Sync host step used by [`crate::graph::GraphBuilder::call`].
pub(crate) struct SyncCall<F> {
    pub(crate) func: F,
}

#[async_trait]
impl<F> GraphCall for SyncCall<F>
where
    F: Fn(&Client, &Value) -> Result<Value, GraphError> + Send + Sync,
{
    async fn call(&self, client: &Client, state: &Value) -> Result<Value, GraphError> {
        (self.func)(client, state)
    }
}
