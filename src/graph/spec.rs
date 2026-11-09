//! JSON graph spec for bindings. Host closures are not part of this shape.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::AgentSpec;
use crate::judge::JudgeConfig;

use super::error::GraphError;

fn default_visits() -> u32 {
    1
}

/// One built-in step. An unknown `kind` fails at decode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepSpec {
    /// [`Client::run_agent`](crate::Client::run_agent). `prompt` is a template.
    Agent { spec: AgentSpec, prompt: String },
    /// [`Client::complete`](crate::Client::complete). `prompt` is a template.
    Complete { prompt: String },
    /// [`Client::judge`](crate::Client::judge). `input` is a state path.
    Judge { config: JudgeConfig, input: String },
    /// [`Client::embed`](crate::Client::embed). `input` is a path to a string.
    Embed {
        model: String,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dimensions: Option<usize>,
    },
    /// One registered tool. `args` is a path to a JSON object.
    Tool { name: String, args: String },
    /// Fill `template` and write the string.
    Map { template: String },
}

/// A data route. The path is read from the value the source node just wrote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhenSpec {
    /// Path inside the source node's output. Empty compares the whole value.
    pub path: String,
    /// JSON value that selects `target`.
    pub equals: Value,
    /// Node that becomes active when the comparison matches.
    pub target: String,
}

/// One node in a [`GraphSpec`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSpec {
    /// Node id. This is also the write key when `write` is omitted.
    pub id: String,
    /// State key this node writes. Defaults to `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<String>,
    /// Maximum runs. The default is 1.
    #[serde(default = "default_visits")]
    pub visits: u32,
    /// Nodes that must finish before this node can run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
    /// The component this node calls.
    pub step: StepSpec,
    /// Ordered data routes. The first match wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<WhenSpec>,
    /// Target when no `when` entry matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<String>,
}

/// A serializable graph. Load it with [`GraphSpec::from_json`], then [`Graph::from_spec`](super::Graph::from_spec).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSpec {
    /// Graph name. Used in traces.
    pub name: String,
    /// Top-level input keys that must be present before the first node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require: Vec<String>,
    /// State path returned by the run.
    pub output: String,
    /// Nodes in builder order.
    pub nodes: Vec<NodeSpec>,
}

const STEP_KINDS: &[&str] = &["agent", "complete", "judge", "embed", "tool", "map"];

impl GraphSpec {
    /// Decode a graph. An unknown step `kind` returns [`GraphError::UnknownStep`].
    pub fn from_json(text: &str) -> Result<Self, GraphError> {
        let value: Value = serde_json::from_str(text).map_err(|_| GraphError::InvalidSpec)?;
        let Some(nodes) = value.get("nodes").and_then(Value::as_array) else {
            return Err(GraphError::InvalidSpec);
        };
        for node in nodes {
            let kind = node
                .get("step")
                .and_then(|step| step.get("kind"))
                .and_then(Value::as_str);
            match kind {
                Some(kind) if STEP_KINDS.contains(&kind) => {}
                Some(_) => return Err(GraphError::UnknownStep),
                None => return Err(GraphError::InvalidSpec),
            }
        }
        from_value(value)
    }

    /// Encode this spec.
    pub fn to_json(&self) -> Result<String, GraphError> {
        serde_json::to_string(self).map_err(|_| GraphError::InvalidSpec)
    }
}

fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, GraphError> {
    serde_json::from_value(value).map_err(|_| GraphError::InvalidSpec)
}
