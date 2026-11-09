//! Build checks: ids, edges, cycles, and static write keys.

use std::collections::{HashMap, HashSet};

use super::spec::{GraphSpec, NodeSpec, StepSpec, WhenSpec};
use super::state::valid_key;
use super::step::{Route, Step};
use super::{Graph, GraphError, Node};

pub(crate) fn compile(
    name: String,
    require: Vec<String>,
    output: Option<String>,
    nodes: Vec<Node>,
) -> Result<Graph, GraphError> {
    if nodes.is_empty() {
        return Err(GraphError::Empty);
    }
    let Some(output) = output.filter(|path| !path.is_empty()) else {
        return Err(GraphError::MissingOutput);
    };
    let mut index = HashMap::with_capacity(nodes.len());
    for (place, node) in nodes.iter().enumerate() {
        if index.insert(node.id.clone(), place).is_some() {
            return Err(GraphError::DuplicateNode(node.id.clone()));
        }
    }
    for node in &nodes {
        if node.visits == 0 {
            return Err(GraphError::InvalidVisits(node.id.clone()));
        }
        for dep in &node.depends {
            if !index.contains_key(dep) {
                return Err(GraphError::UnknownNode(dep.clone()));
            }
        }
        for route in &node.routes {
            if !index.contains_key(route.target()) {
                return Err(GraphError::UnknownNode(route.target().to_string()));
            }
        }
        if let Some(target) = &node.otherwise
            && !index.contains_key(target)
        {
            return Err(GraphError::UnknownNode(target.clone()));
        }
        if let Step::Judge { config, .. } = &node.step {
            config.validate()?;
        }
    }
    if depends_cycle(&nodes) {
        return Err(GraphError::DependsCycle);
    }
    if unbound_cycle(&nodes) {
        return Err(GraphError::UnboundCycle);
    }
    let root = output.split('.').next().unwrap_or("");
    if !nodes.iter().any(|node| node.write == root) {
        return Err(GraphError::OutputUnwritten(output));
    }
    check_static_writes(&nodes)?;

    let mut incoming = HashSet::new();
    for node in &nodes {
        for route in &node.routes {
            incoming.insert(route.target().to_string());
        }
        if let Some(target) = &node.otherwise {
            incoming.insert(target.clone());
        }
    }

    Ok(Graph {
        name,
        require,
        output,
        nodes,
        index,
        incoming,
    })
}

fn depends_cycle(nodes: &[Node]) -> bool {
    let index = index_of(nodes);
    let mut color = vec![0u8; nodes.len()];
    for start in 0..nodes.len() {
        if color[start] == 0 && cycle_from(start, nodes, &index, &mut color, EdgeKind::Depends) {
            return true;
        }
    }
    false
}

fn unbound_cycle(nodes: &[Node]) -> bool {
    let index = index_of(nodes);
    let mut color = vec![0u8; nodes.len()];
    let mut stack = Vec::new();
    for start in 0..nodes.len() {
        if color[start] == 0 && unbound_from(start, nodes, &index, &mut color, &mut stack) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
enum EdgeKind {
    Depends,
    Any,
}

fn cycle_from(
    current: usize,
    nodes: &[Node],
    index: &HashMap<&str, usize>,
    color: &mut [u8],
    kind: EdgeKind,
) -> bool {
    color[current] = 1;
    for next in neighbors(nodes, current, index, kind) {
        if color[next] == 1 || (color[next] == 0 && cycle_from(next, nodes, index, color, kind)) {
            return true;
        }
    }
    color[current] = 2;
    false
}

fn unbound_from(
    current: usize,
    nodes: &[Node],
    index: &HashMap<&str, usize>,
    color: &mut [u8],
    stack: &mut Vec<usize>,
) -> bool {
    color[current] = 1;
    stack.push(current);
    for next in neighbors(nodes, current, index, EdgeKind::Any) {
        if color[next] == 1 {
            let start = stack.iter().position(|id| *id == next).unwrap_or(0);
            let capped = stack[start..].iter().all(|id| nodes[*id].visits == 1);
            if capped {
                return true;
            }
        } else if color[next] == 0 && unbound_from(next, nodes, index, color, stack) {
            return true;
        }
    }
    stack.pop();
    color[current] = 2;
    false
}

fn neighbors(
    nodes: &[Node],
    current: usize,
    index: &HashMap<&str, usize>,
    kind: EdgeKind,
) -> Vec<usize> {
    let node = &nodes[current];
    let mut out = Vec::new();
    for dep in &node.depends {
        if let Some(place) = index.get(dep.as_str()) {
            out.push(*place);
        }
    }
    if matches!(kind, EdgeKind::Any) {
        for route in &node.routes {
            if let Some(place) = index.get(route.target()) {
                out.push(*place);
            }
        }
        if let Some(target) = &node.otherwise
            && let Some(place) = index.get(target.as_str())
        {
            out.push(*place);
        }
    }
    out
}

fn index_of(nodes: &[Node]) -> HashMap<&str, usize> {
    nodes
        .iter()
        .enumerate()
        .map(|(place, node)| (node.id.as_str(), place))
        .collect()
}

/// Nodes that run without a route form dependency waves.
/// Two of them in one wave cannot share a write key.
/// A route can still put two writers in one wave. The runner reports that.
fn check_static_writes(nodes: &[Node]) -> Result<(), GraphError> {
    let incoming = incoming_ids(nodes);
    let index = index_of(nodes);
    let mut memo = HashMap::new();
    let mut stack = HashSet::new();
    let mut static_places = Vec::new();
    for place in 0..nodes.len() {
        if is_static(place, nodes, &incoming, &index, &mut memo, &mut stack) {
            static_places.push(place);
        }
    }
    let mut wave_of: HashMap<&str, usize> = HashMap::new();
    let mut remaining = static_places;
    let mut wave = 0usize;
    while !remaining.is_empty() {
        let (ready, rest): (Vec<_>, Vec<_>) = remaining.into_iter().partition(|place| {
            nodes[*place]
                .depends
                .iter()
                .all(|dep| wave_of.contains_key(dep.as_str()))
        });
        if ready.is_empty() {
            return Err(GraphError::DependsCycle);
        }
        let mut keys: HashMap<&str, ()> = HashMap::new();
        for place in &ready {
            let node = &nodes[*place];
            if keys.insert(node.write.as_str(), ()).is_some() {
                return Err(GraphError::StateConflict(node.write.clone()));
            }
            wave_of.insert(node.id.as_str(), wave);
        }
        wave += 1;
        remaining = rest;
    }
    Ok(())
}

fn is_static(
    place: usize,
    nodes: &[Node],
    incoming: &HashMap<&str, ()>,
    index: &HashMap<&str, usize>,
    memo: &mut HashMap<usize, bool>,
    stack: &mut HashSet<usize>,
) -> bool {
    if let Some(known) = memo.get(&place) {
        return *known;
    }
    if !stack.insert(place) {
        return false;
    }
    let node = &nodes[place];
    let ok = !incoming.contains_key(node.id.as_str())
        && node.depends.iter().all(|dep| {
            index
                .get(dep.as_str())
                .is_some_and(|dep_place| is_static(*dep_place, nodes, incoming, index, memo, stack))
        });
    stack.remove(&place);
    memo.insert(place, ok);
    ok
}

fn incoming_ids(nodes: &[Node]) -> HashMap<&str, ()> {
    let mut incoming = HashMap::new();
    for node in nodes {
        for route in &node.routes {
            incoming.insert(route.target(), ());
        }
        if let Some(target) = &node.otherwise {
            incoming.insert(target.as_str(), ());
        }
    }
    incoming
}

impl Graph {
    /// Encode the built-in steps. A host closure returns [`GraphError::NotSerializable`].
    pub fn to_spec(&self) -> Result<GraphSpec, GraphError> {
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            if node.step.is_call() || node.routes.iter().any(Route::is_closure) {
                return Err(GraphError::NotSerializable);
            }
            nodes.push(NodeSpec {
                id: node.id.clone(),
                write: (node.write != node.id).then(|| node.write.clone()),
                visits: node.visits,
                depends: node.depends.clone(),
                step: step_spec(&node.step)?,
                when: when_specs(&node.routes)?,
                otherwise: node.otherwise.clone(),
            });
        }
        Ok(GraphSpec {
            name: self.name.clone(),
            require: self.require.clone(),
            output: self.output.clone(),
            nodes,
        })
    }

    /// Build a graph from a spec. The same checks as [`GraphBuilder::build`] run.
    pub fn from_spec(spec: GraphSpec) -> Result<Self, GraphError> {
        let mut nodes = Vec::with_capacity(spec.nodes.len());
        for node in spec.nodes {
            let id = node.id;
            if !valid_key(&id) {
                return Err(if id.is_empty() {
                    GraphError::EmptyNodeId
                } else {
                    GraphError::BadId(id)
                });
            }
            let write = node.write.unwrap_or_else(|| id.clone());
            if !valid_key(&write) {
                return Err(GraphError::BadId(write));
            }
            nodes.push(Node {
                id,
                step: step_from_spec(node.step),
                write,
                depends: node.depends,
                visits: node.visits,
                routes: node
                    .when
                    .into_iter()
                    .map(|when| Route::Equals {
                        path: when.path,
                        expected: when.equals,
                        target: when.target,
                    })
                    .collect(),
                otherwise: node.otherwise,
            });
        }
        compile(spec.name, spec.require, Some(spec.output), nodes)
    }
}

fn step_spec(step: &Step) -> Result<StepSpec, GraphError> {
    Ok(match step {
        Step::Agent { spec, prompt } => StepSpec::Agent {
            spec: spec.clone(),
            prompt: prompt.clone(),
        },
        Step::Complete { prompt } => StepSpec::Complete {
            prompt: prompt.clone(),
        },
        Step::Judge { config, input } => StepSpec::Judge {
            config: config.clone(),
            input: input.clone(),
        },
        Step::Embed {
            model,
            input,
            dimensions,
        } => StepSpec::Embed {
            model: model.clone(),
            input: input.clone(),
            dimensions: *dimensions,
        },
        Step::Tool { name, args } => StepSpec::Tool {
            name: name.clone(),
            args: args.clone(),
        },
        Step::Map { template } => StepSpec::Map {
            template: template.clone(),
        },
        Step::Call { .. } => return Err(GraphError::NotSerializable),
    })
}

fn when_specs(routes: &[Route]) -> Result<Vec<WhenSpec>, GraphError> {
    let mut out = Vec::new();
    for route in routes {
        match route {
            Route::Equals {
                path,
                expected,
                target,
            } => out.push(WhenSpec {
                path: path.clone(),
                equals: expected.clone(),
                target: target.clone(),
            }),
            Route::Pred { .. } => return Err(GraphError::NotSerializable),
        }
    }
    Ok(out)
}

fn step_from_spec(step: StepSpec) -> Step {
    match step {
        StepSpec::Agent { spec, prompt } => Step::Agent { spec, prompt },
        StepSpec::Complete { prompt } => Step::Complete { prompt },
        StepSpec::Judge { config, input } => Step::Judge { config, input },
        StepSpec::Embed {
            model,
            input,
            dimensions,
        } => Step::Embed {
            model,
            input,
            dimensions,
        },
        StepSpec::Tool { name, args } => Step::Tool { name, args },
        StepSpec::Map { template } => Step::Map { template },
    }
}
