//! Fluent builder. `build` checks ids, edges, cycles, and static write keys.

use std::sync::Arc;

use serde_json::Value;

use crate::agents::AgentSpec;
use crate::judge::JudgeConfig;

use super::state::valid_key;
use super::step::{Route, Step, SyncCall};
use super::{Graph, GraphCall, GraphError, Node};

/// Builds a [`Graph`]. Chain the node methods, then call [`GraphBuilder::build`].
pub struct GraphBuilder {
    name: String,
    require: Vec<String>,
    output: Option<String>,
    nodes: Vec<Node>,
    faults: Vec<GraphError>,
}

impl GraphBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            require: Vec::new(),
            output: None,
            nodes: Vec::new(),
            faults: Vec::new(),
        }
    }

    /// Require a top-level input key.
    #[must_use]
    pub fn require(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        if !valid_key(&key) {
            self.faults.push(GraphError::BadId(key));
        } else {
            self.require.push(key);
        }
        self
    }

    /// Run an agent. `prompt` is a `{path}` template.
    #[must_use]
    pub fn agent(self, id: impl Into<String>, spec: AgentSpec, prompt: impl Into<String>) -> Self {
        self.push(
            id,
            Step::Agent {
                spec,
                prompt: prompt.into(),
            },
        )
    }

    /// Run one chat turn. `prompt` is a `{path}` template.
    #[must_use]
    pub fn complete(self, id: impl Into<String>, prompt: impl Into<String>) -> Self {
        self.push(
            id,
            Step::Complete {
                prompt: prompt.into(),
            },
        )
    }

    /// Run a judge. `input` is a state path. The node writes the verdict object.
    #[must_use]
    pub fn judge(
        self,
        id: impl Into<String>,
        config: JudgeConfig,
        input: impl Into<String>,
    ) -> Self {
        self.push(
            id,
            Step::Judge {
                config,
                input: input.into(),
            },
        )
    }

    /// Embed the string at `input`.
    #[must_use]
    pub fn embed(
        self,
        id: impl Into<String>,
        model: impl Into<String>,
        input: impl Into<String>,
    ) -> Self {
        self.push(
            id,
            Step::Embed {
                model: model.into(),
                input: input.into(),
                dimensions: None,
            },
        )
    }

    /// Set the embedding dimensions for an embed node.
    #[must_use]
    pub fn dimensions(mut self, id: impl Into<String>, dimensions: usize) -> Self {
        let id = id.into();
        match self.nodes.iter_mut().find(|node| node.id == id) {
            Some(node) => match node.step.embed_mut() {
                Some(slot) => *slot = Some(dimensions),
                None => self.faults.push(GraphError::BadStep(id)),
            },
            None => self.faults.push(GraphError::UnknownNode(id)),
        }
        self
    }

    /// Call one registered tool. `args` is a path to a JSON object.
    #[must_use]
    pub fn tool(
        self,
        id: impl Into<String>,
        name: impl Into<String>,
        args: impl Into<String>,
    ) -> Self {
        self.push(
            id,
            Step::Tool {
                name: name.into(),
                args: args.into(),
            },
        )
    }

    /// Write a filled template string.
    #[must_use]
    pub fn map(self, id: impl Into<String>, template: impl Into<String>) -> Self {
        self.push(
            id,
            Step::Map {
                template: template.into(),
            },
        )
    }

    /// Run host code. This step is absent from a JSON spec.
    #[must_use]
    pub fn call<F>(self, id: impl Into<String>, func: F) -> Self
    where
        F: Fn(&crate::Client, &Value) -> Result<Value, GraphError> + Send + Sync + 'static,
    {
        self.call_with(id, Arc::new(SyncCall { func }))
    }

    /// Run a [`GraphCall`]. Use this for an async host step.
    #[must_use]
    pub fn call_with(self, id: impl Into<String>, runner: Arc<dyn GraphCall>) -> Self {
        self.push(id, Step::Call { runner })
    }

    /// Write this node to `key` instead of its id.
    #[must_use]
    pub fn write(mut self, id: impl Into<String>, key: impl Into<String>) -> Self {
        let id = id.into();
        let key = key.into();
        if !valid_key(&key) {
            self.faults.push(GraphError::BadId(key));
            return self;
        }
        match self.nodes.iter_mut().find(|node| node.id == id) {
            Some(node) => node.write = key,
            None => self.faults.push(GraphError::UnknownNode(id)),
        }
        self
    }

    /// Nodes that must finish before `id` runs.
    #[must_use]
    pub fn depends(
        mut self,
        id: impl Into<String>,
        deps: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let id = id.into();
        let deps: Vec<String> = deps.into_iter().map(Into::into).collect();
        match self.nodes.iter_mut().find(|node| node.id == id) {
            Some(node) => node.depends = deps,
            None => self.faults.push(GraphError::UnknownNode(id)),
        }
        self
    }

    /// Maximum times `id` may run. The default is 1.
    #[must_use]
    pub fn visits(mut self, id: impl Into<String>, visits: u32) -> Self {
        let id = id.into();
        if visits == 0 {
            self.faults.push(GraphError::InvalidVisits(id));
            return self;
        }
        match self.nodes.iter_mut().find(|node| node.id == id) {
            Some(node) => node.visits = visits,
            None => self.faults.push(GraphError::UnknownNode(id)),
        }
        self
    }

    /// Select `target` when the path inside this node's output equals `equals`.
    ///
    /// The path is relative to the value the node just wrote. `"pass"` reads
    /// the `pass` field of a judge verdict. The first matching rule wins.
    #[must_use]
    pub fn when(
        mut self,
        from: impl Into<String>,
        path: impl Into<String>,
        equals: Value,
        target: impl Into<String>,
    ) -> Self {
        let from = from.into();
        let route = Route::Equals {
            path: path.into(),
            expected: equals,
            target: target.into(),
        };
        self.push_route(from, route);
        self
    }

    /// Select `target` when `pred` returns true. The predicate sees the full state.
    ///
    /// This route is absent from a JSON spec.
    #[must_use]
    pub fn when_fn<F>(mut self, from: impl Into<String>, target: impl Into<String>, pred: F) -> Self
    where
        F: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        let from = from.into();
        let route = Route::Pred {
            pred: Arc::new(pred),
            target: target.into(),
        };
        self.push_route(from, route);
        self
    }

    /// Select `target` when no [`when`](Self::when) rule matches.
    #[must_use]
    pub fn otherwise(mut self, from: impl Into<String>, target: impl Into<String>) -> Self {
        let from = from.into();
        let target = target.into();
        match self.nodes.iter_mut().find(|node| node.id == from) {
            Some(node) => node.otherwise = Some(target),
            None => self.faults.push(GraphError::UnknownNode(from)),
        }
        self
    }

    /// State path the run returns.
    #[must_use]
    pub fn output(mut self, path: impl Into<String>) -> Self {
        self.output = Some(path.into());
        self
    }

    /// Validate the graph.
    pub fn build(self) -> Result<Graph, GraphError> {
        if let Some(error) = self.faults.into_iter().next() {
            return Err(error);
        }
        super::check::compile(self.name, self.require, self.output, self.nodes)
    }

    fn push(mut self, id: impl Into<String>, step: Step) -> Self {
        let id = id.into();
        if !valid_key(&id) {
            self.faults.push(if id.is_empty() {
                GraphError::EmptyNodeId
            } else {
                GraphError::BadId(id)
            });
            return self;
        }
        if self.nodes.iter().any(|node| node.id == id) {
            self.faults.push(GraphError::DuplicateNode(id));
            return self;
        }
        self.nodes.push(Node {
            id: id.clone(),
            step,
            write: id,
            depends: Vec::new(),
            visits: 1,
            routes: Vec::new(),
            otherwise: None,
        });
        self
    }

    fn push_route(&mut self, from: String, route: Route) {
        match self.nodes.iter_mut().find(|node| node.id == from) {
            Some(node) => node.routes.push(route),
            None => self.faults.push(GraphError::UnknownNode(from)),
        }
    }
}
