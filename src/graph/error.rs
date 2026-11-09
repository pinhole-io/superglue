//! Errors from building and running a graph.

use thiserror::Error;

use crate::chat::ChatError;
use crate::embeddings::EmbedError;
use crate::judge::JudgeError;
use crate::tools::ToolInvokeError;

/// A graph build or run failure.
///
/// Messages name the node, the path, or the error kind. They do not include
/// state values, prompts, or provider bodies beyond the source error.
#[derive(Debug, Error)]
pub enum GraphError {
    /// The builder has no nodes.
    #[error("graph has no nodes")]
    Empty,
    /// A node id or a required key is empty.
    #[error("node id is empty")]
    EmptyNodeId,
    /// The same node id was added twice.
    #[error("duplicate node id: {0}")]
    DuplicateNode(String),
    /// `depends` or `when` names a node that was not added.
    #[error("unknown node: {0}")]
    UnknownNode(String),
    /// A node id or write key cannot be used in a state path.
    #[error("node id {0} is not a valid key")]
    BadId(String),
    /// The run input is not a JSON object.
    #[error("graph input must be a JSON object")]
    InputType,
    /// A required input key is absent.
    #[error("missing input key: {0}")]
    MissingInput(String),
    /// A template or route path is absent.
    #[error("missing state path: {0}")]
    MissingKey(String),
    /// A path exists and has the wrong JSON type.
    #[error("state path has the wrong type: {0}")]
    BadType(String),
    /// A template has an unclosed `{`.
    #[error("template is invalid")]
    BadTemplate,
    /// `depends` edges form a cycle, so the graph cannot start.
    #[error("a dependency cycle cannot start")]
    DependsCycle,
    /// A cycle exists and every node on it has `visits == 1`.
    #[error("a cycle needs visits greater than 1")]
    UnboundCycle,
    /// Two nodes in one wave write the same key.
    #[error("write key {0} is shared by nodes in one wave")]
    StateConflict(String),
    /// A route selected a node that has already used its visit cap.
    #[error("node {0} exceeded its visit limit")]
    VisitLimit(String),
    /// `visits` was set to 0.
    #[error("visits for {0} must be at least 1")]
    InvalidVisits(String),
    /// A modifier does not match the node step.
    #[error("node {0} does not accept that setting")]
    BadStep(String),
    /// The output key was not written because its node was skipped.
    #[error("output {0} was skipped")]
    OutputSkipped(String),
    /// No node writes the output key.
    #[error("output {0} is not written by any node")]
    OutputUnwritten(String),
    /// `output` was not set.
    #[error("graph has no output")]
    MissingOutput,
    /// `to_spec` saw a host closure.
    #[error("graph spec cannot include a host closure")]
    NotSerializable,
    /// The JSON step kind is not one of the built-in steps.
    #[error("unknown graph step kind")]
    UnknownStep,
    /// The JSON graph could not be decoded.
    #[error("graph spec is invalid")]
    InvalidSpec,
    /// The client cancellation token was cancelled before the next wave.
    #[error("graph run cancelled")]
    Cancelled,
    /// The run stopped and the output was neither written nor skipped.
    #[error("graph stopped before the output was written")]
    Stalled,
    /// An agent or complete step failed.
    #[error(transparent)]
    Chat(#[from] ChatError),
    /// A judge step failed.
    #[error(transparent)]
    Judge(#[from] JudgeError),
    /// An embed step failed.
    #[error(transparent)]
    Embed(#[from] EmbedError),
    /// A tool step failed.
    #[error(transparent)]
    Tool(#[from] ToolInvokeError),
}
