//! Mermaid flowchart for one finished graph run.
//!
//! The drawing names nodes and arrows. It does not include prompts, tool
//! arguments, or the values a node wrote.

use std::collections::HashSet;
use std::fmt;

use serde_json::Value;

use super::Graph;

/// One finished graph run.
pub struct GraphRun {
    value: Value,
    mermaid: String,
}

impl GraphRun {
    pub(crate) fn new(value: Value, mermaid: String) -> Self {
        Self { value, mermaid }
    }

    /// JSON value at the graph output path.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Mermaid flowchart of this run.
    ///
    /// A solid arrow was followed. A dotted arrow was not.
    /// A route label is the spec check, not the value the node wrote.
    #[must_use]
    pub fn mermaid(&self) -> &str {
        &self.mermaid
    }

    /// Drop the diagram and return the output value.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.value
    }
}

impl fmt::Debug for GraphRun {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphRun")
            .field("mermaid_chars", &self.mermaid.len())
            .finish_non_exhaustive()
    }
}

pub(crate) fn render(graph: &Graph, runs: &[u32], taken: &HashSet<(usize, usize)>) -> String {
    let mut lines = vec![
        "flowchart TD".to_string(),
        format!("%% {}", plain(&graph.name)),
        format!("%% output {}", plain(&graph.output)),
    ];
    for (place, node) in graph.nodes.iter().enumerate() {
        let label = plain(&format!(
            "{} · {} · {}",
            node.id,
            node.step.kind_name(),
            status(runs[place])
        ));
        lines.push(format!("  n{place}[\"{label}\"]"));
    }
    for (to, node) in graph.nodes.iter().enumerate() {
        for dep in &node.depends {
            let Some(from) = graph.index.get(dep).copied() else {
                continue;
            };
            let arrow = if runs[from] > 0 && runs[to] > 0 {
                "-->"
            } else {
                "-.->"
            };
            lines.push(format!("  n{from} {arrow} n{to}"));
        }
    }
    for (from, node) in graph.nodes.iter().enumerate() {
        for (slot, route) in node.routes.iter().enumerate() {
            let Some(to) = graph.index.get(route.target()).copied() else {
                continue;
            };
            lines.push(arrow_line(
                from,
                to,
                taken.contains(&(from, slot)),
                &route.diagram_label(),
            ));
        }
        if let Some(target) = &node.otherwise {
            let Some(to) = graph.index.get(target).copied() else {
                continue;
            };
            let slot = node.routes.len();
            lines.push(arrow_line(
                from,
                to,
                taken.contains(&(from, slot)),
                "otherwise",
            ));
        }
    }
    let ran = class_list(runs, true);
    let skipped = class_list(runs, false);
    if !ran.is_empty() {
        lines.push("  classDef ran stroke:#1f6f4a,stroke-width:2px".to_string());
        lines.push(format!("  class {ran} ran"));
    }
    if !skipped.is_empty() {
        lines.push("  classDef skipped stroke:#8a8a8a,stroke-dasharray: 4 3".to_string());
        lines.push(format!("  class {skipped} skipped"));
    }
    lines.join("\n")
}

fn status(runs: u32) -> String {
    if runs == 0 {
        "skipped".to_string()
    } else if runs == 1 {
        "ran".to_string()
    } else {
        format!("ran {runs}")
    }
}

fn class_list(runs: &[u32], ran: bool) -> String {
    runs.iter()
        .enumerate()
        .filter(|(_, count)| (**count > 0) == ran)
        .map(|(place, _)| format!("n{place}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn arrow_line(from: usize, to: usize, followed: bool, label: &str) -> String {
    let arrow = if followed { "-->" } else { "-.->" };
    format!("  n{from} {arrow}|\"{}\"| n{to}", plain(label))
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '"' | '|' | '[' | ']' | '{' | '}' | '#' => ' ',
            other if other.is_control() => ' ',
            other => other,
        })
        .collect()
}
