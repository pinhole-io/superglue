//! Wave runner. Ready nodes in one wave start together.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};
use tokio::task::JoinSet;

use crate::client::Client;

use super::state::lookup;
use super::trace::{self, GraphRun};
use super::{Graph, GraphError};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Active,
}

struct RunState {
    status: Vec<Status>,
    runs: Vec<u32>,
}

impl Graph {
    pub(crate) async fn execute(
        &self,
        client: &Client,
        input: Value,
    ) -> Result<GraphRun, GraphError> {
        let Value::Object(mut state) = input else {
            return Err(GraphError::InputType);
        };
        for key in &self.require {
            if !state.contains_key(key) {
                return Err(GraphError::MissingInput(key.clone()));
            }
        }

        let mut run = RunState {
            status: vec![Status::Pending; self.nodes.len()],
            runs: vec![0; self.nodes.len()],
        };
        let mut taken = HashSet::new();
        for (place, node) in self.nodes.iter().enumerate() {
            if node.depends.is_empty() && !self.incoming.contains(&node.id) {
                run.status[place] = Status::Active;
            }
        }

        loop {
            activate_static(self, &mut run);
            let ready = ready_nodes(self, &run)?;
            if ready.is_empty() {
                break;
            }
            if client.is_graph_cancelled() {
                return Err(GraphError::Cancelled);
            }
            let snapshot = Value::Object(state.clone());
            let patches = run_wave(client, self, &ready, snapshot).await?;
            apply_patches(&mut state, &patches)?;
            let merged = Value::Object(state.clone());
            select_routes(self, &ready, &patches, &merged, &mut run, &mut taken)?;
        }

        let value = read_output(self, &run, &Value::Object(state))?;
        let mermaid = trace::render(self, &run.runs, &taken);
        Ok(GraphRun::new(value, mermaid))
    }
}

fn activate_static(graph: &Graph, run: &mut RunState) {
    for (place, node) in graph.nodes.iter().enumerate() {
        if run.runs[place] != 0 || run.status[place] == Status::Active {
            continue;
        }
        if graph.incoming.contains(&node.id) {
            continue;
        }
        let deps_done = node.depends.iter().all(|dep| {
            graph
                .index
                .get(dep)
                .is_some_and(|dep_place| run.runs[*dep_place] > 0)
        });
        if deps_done {
            run.status[place] = Status::Active;
        }
    }
}

fn ready_nodes(graph: &Graph, run: &RunState) -> Result<Vec<usize>, GraphError> {
    let mut ready = Vec::new();
    for (place, node) in graph.nodes.iter().enumerate() {
        if run.status[place] != Status::Active {
            continue;
        }
        let waiting = node.depends.iter().any(|dep| {
            graph
                .index
                .get(dep)
                .is_none_or(|dep_place| run.runs[*dep_place] == 0)
        });
        if waiting {
            continue;
        }
        if run.runs[place] >= node.visits {
            return Err(GraphError::VisitLimit(node.id.clone()));
        }
        ready.push(place);
    }
    Ok(ready)
}

async fn run_wave(
    client: &Client,
    graph: &Graph,
    ready: &[usize],
    snapshot: Value,
) -> Result<Vec<(usize, String, Value)>, GraphError> {
    let mut set = JoinSet::new();
    for place in ready {
        let place = *place;
        let node = &graph.nodes[place];
        let step = node.step.clone();
        let write = node.write.clone();
        let state = snapshot.clone();
        let client = client.clone();
        set.spawn(async move {
            let value = step.execute(&client, &state).await?;
            Ok::<_, GraphError>((place, write, value))
        });
    }
    let mut patches = Vec::with_capacity(ready.len());
    while let Some(joined) = set.join_next().await {
        patches.push(joined.map_err(|_| GraphError::Stalled)??);
    }
    Ok(patches)
}

fn apply_patches(
    state: &mut Map<String, Value>,
    patches: &[(usize, String, Value)],
) -> Result<(), GraphError> {
    let mut seen = HashSet::new();
    for (_, key, _) in patches {
        if !seen.insert(key.clone()) {
            return Err(GraphError::StateConflict(key.clone()));
        }
    }
    for (_, key, value) in patches {
        state.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn select_routes(
    graph: &Graph,
    ready: &[usize],
    patches: &[(usize, String, Value)],
    state: &Value,
    run: &mut RunState,
    taken: &mut HashSet<(usize, usize)>,
) -> Result<(), GraphError> {
    let written: HashMap<usize, &Value> = patches
        .iter()
        .map(|(place, _, value)| (*place, value))
        .collect();
    for place in ready {
        let node = &graph.nodes[*place];
        run.runs[*place] += 1;
        run.status[*place] = Status::Pending;
        let Some(value) = written.get(place) else {
            continue;
        };
        let mut selected: Option<usize> = None;
        for (slot, route) in node.routes.iter().enumerate() {
            if route.matches(value, state)? {
                selected = Some(slot);
                break;
            }
        }
        let target = if let Some(slot) = selected {
            taken.insert((*place, slot));
            Some(node.routes[slot].target().to_string())
        } else if node.otherwise.is_some() {
            taken.insert((*place, node.routes.len()));
            node.otherwise.clone()
        } else {
            None
        };
        if let Some(target) = target
            && let Some(target_place) = graph.index.get(&target)
        {
            run.status[*target_place] = Status::Active;
        }
    }
    Ok(())
}

fn read_output(graph: &Graph, run: &RunState, state: &Value) -> Result<Value, GraphError> {
    let root = graph.output.split('.').next().unwrap_or("");
    let any_ran = graph
        .nodes
        .iter()
        .enumerate()
        .any(|(place, node)| node.write == root && run.runs[place] > 0);
    if !any_ran {
        return Err(GraphError::OutputSkipped(graph.output.clone()));
    }
    lookup(state, &graph.output)
        .cloned()
        .map_err(|_| GraphError::Stalled)
}
