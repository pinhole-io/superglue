//! Graph builder and wave runner.
//!
//! A graph connects superglue calls with JSON state. Build one with
//! [`Graph::builder`], or load a [`GraphSpec`] and run it with
//! [`Client::run_graph`](crate::Client::run_graph).
//!
//! This is not a durable workflow. A crash does not resume the graph.
//! Each agent and complete node still uses that call's audit path.

mod builder;
mod check;
mod error;
mod run;
mod spec;
mod state;
mod step;

pub use builder::GraphBuilder;
pub use error::GraphError;
pub use spec::GraphSpec;
pub use step::GraphCall;

use std::collections::{HashMap, HashSet};
use std::fmt;

use step::{Route, Step};

/// A validated graph.
pub struct Graph {
    name: String,
    require: Vec<String>,
    output: String,
    pub(crate) nodes: Vec<Node>,
    index: HashMap<String, usize>,
    /// Nodes selected by a `when` or `otherwise` route.
    incoming: HashSet<String>,
}

pub(crate) struct Node {
    id: String,
    step: Step,
    write: String,
    depends: Vec<String>,
    visits: u32,
    routes: Vec<Route>,
    otherwise: Option<String>,
}

impl Graph {
    /// Start a builder. `name` is the trace name.
    #[must_use]
    pub fn builder(name: impl Into<String>) -> GraphBuilder {
        GraphBuilder::new(name)
    }

    /// Trace name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for Graph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Graph")
            .field("name", &self.name)
            .field("output", &self.output)
            .field("nodes", &self.nodes.len())
            .finish()
    }
}
