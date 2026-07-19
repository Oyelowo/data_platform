//! Simple pattern matcher for labeled property graph chains.

use std::collections::BTreeSet;

use crate::id::{InternalEdgeId, InternalNodeId};
use crate::index::adjacency::Direction;

/// A single step in a pattern chain.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternStep {
    /// Expected label(s) for the node at the start of this step. Empty means any.
    pub node_labels: BTreeSet<String>,
    /// Direction of the edge relative to the current node.
    pub direction: Direction,
    /// Expected edge label. Empty means any.
    pub edge_label: Option<String>,
    /// Expected label(s) for the node at the other end of the edge. Empty means any.
    pub next_node_labels: BTreeSet<String>,
}

impl PatternStep {
    /// Create a pattern step.
    pub fn new(
        node_labels: impl IntoIterator<Item = impl Into<String>>,
        direction: Direction,
        edge_label: Option<impl Into<String>>,
        next_node_labels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            node_labels: node_labels.into_iter().map(Into::into).collect(),
            direction,
            edge_label: edge_label.map(Into::into),
            next_node_labels: next_node_labels.into_iter().map(Into::into).collect(),
        }
    }
}

/// A binding of node variables to internal node ids.
pub type Binding = Vec<InternalNodeId>;

/// Context for pattern matching.
pub struct PatternContext<'a> {
    /// Return labels for a node id.
    pub node_labels_fn: &'a dyn Fn(InternalNodeId) -> Option<BTreeSet<String>>,
    /// Return incident edges for a node in a direction.
    pub edges_fn: &'a dyn Fn(InternalNodeId, Direction) -> Vec<InternalEdgeId>,
    /// Resolve the other endpoint of an edge.
    pub endpoint_fn: &'a dyn Fn(InternalNodeId, InternalEdgeId, Direction) -> Option<InternalNodeId>,
    /// Resolve the label of an edge.
    pub edge_label_fn: &'a dyn Fn(InternalEdgeId) -> Option<String>,
}

/// Match a chain of `PatternStep`s starting from `start`.
///
/// Returns all bindings of length `steps.len() + 1` that satisfy the chain.
pub fn match_pattern(
    start: InternalNodeId,
    steps: &[PatternStep],
    ctx: &PatternContext<'_>,
) -> Vec<Binding> {
    let mut results = Vec::new();
    let mut path = Vec::with_capacity(steps.len() + 1);
    path.push(start);
    if !labels_match(&(ctx.node_labels_fn)(start), &steps.first().map(|s| s.node_labels.clone()).unwrap_or_default()) {
        return results;
    }
    backtrack(start, 0, steps, ctx, &mut path, &mut results);
    results
}

fn backtrack(
    current: InternalNodeId,
    step_index: usize,
    steps: &[PatternStep],
    ctx: &PatternContext<'_>,
    path: &mut Vec<InternalNodeId>,
    results: &mut Vec<Binding>,
) {
    if step_index >= steps.len() {
        results.push(path.clone());
        return;
    }
    let step = &steps[step_index];
    for edge_id in (ctx.edges_fn)(current, step.direction) {
        if let Some(label) = (ctx.edge_label_fn)(edge_id)
            && let Some(ref expected) = step.edge_label
            && label != *expected
        {
            continue;
        }
        if let Some(next) = (ctx.endpoint_fn)(current, edge_id, step.direction) {
            if !labels_match(&(ctx.node_labels_fn)(next), &step.next_node_labels) {
                continue;
            }
            path.push(next);
            backtrack(next, step_index + 1, steps, ctx, path, results);
            path.pop();
        }
    }
}

fn labels_match(actual: &Option<BTreeSet<String>>, expected: &BTreeSet<String>) -> bool {
    if expected.is_empty() {
        return true;
    }
    match actual {
        Some(labels) => expected.iter().all(|l| labels.contains(l)),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Graph {
        node_labels: BTreeMap<InternalNodeId, BTreeSet<String>>,
        edges: BTreeMap<InternalEdgeId, (InternalNodeId, InternalNodeId, String)>,
        adjacency: BTreeMap<InternalNodeId, Vec<InternalEdgeId>>,
    }

    fn g() -> Graph {
        let mut node_labels: BTreeMap<InternalNodeId, BTreeSet<String>> = BTreeMap::new();
        let mut edges: BTreeMap<InternalEdgeId, (InternalNodeId, InternalNodeId, String)> = BTreeMap::new();
        let mut adjacency: BTreeMap<InternalNodeId, Vec<InternalEdgeId>> = BTreeMap::new();
        let n = |i| InternalNodeId(i);
        let e = |i| InternalEdgeId(i);
        node_labels.insert(n(1), ["User".into()].into());
        node_labels.insert(n(2), ["User".into()].into());
        node_labels.insert(n(3), ["Post".into()].into());
        edges.insert(e(1), (n(1), n(2), "FOLLOWS".into()));
        edges.insert(e(2), (n(2), n(3), "WROTE".into()));
        adjacency.entry(n(1)).or_default().push(e(1));
        adjacency.entry(n(2)).or_default().push(e(2));
        Graph { node_labels, edges, adjacency }
    }

    #[test]
    fn match_simple_chain() {
        let graph = g();
        let ctx = PatternContext {
            node_labels_fn: &|n| graph.node_labels.get(&n).cloned(),
            edges_fn: &|n, _d| graph.adjacency.get(&n).cloned().unwrap_or_default(),
            endpoint_fn: &|from, e, d| {
                graph.edges.get(&e).and_then(|(s, t, _)| match d {
                    Direction::Out if *s == from => Some(*t),
                    Direction::In if *t == from => Some(*s),
                    Direction::Both if *s == from => Some(*t),
                    Direction::Both if *t == from => Some(*s),
                    _ => None,
                })
            },
            edge_label_fn: &|e| graph.edges.get(&e).map(|(_, _, l)| l.clone()),
        };
        let steps = vec![
            PatternStep::new(["User"], Direction::Out, Some("FOLLOWS"), ["User"]),
            PatternStep::new(["User"], Direction::Out, Some("WROTE"), ["Post"]),
        ];
        let bindings = match_pattern(InternalNodeId(1), &steps, &ctx);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0], vec![InternalNodeId(1), InternalNodeId(2), InternalNodeId(3)]);
    }

    #[test]
    fn match_no_result() {
        let graph = g();
        let ctx = PatternContext {
            node_labels_fn: &|n| graph.node_labels.get(&n).cloned(),
            edges_fn: &|n, _d| graph.adjacency.get(&n).cloned().unwrap_or_default(),
            endpoint_fn: &|from, e, d| {
                graph.edges.get(&e).and_then(|(s, t, _)| match d {
                    Direction::Out if *s == from => Some(*t),
                    Direction::In if *t == from => Some(*s),
                    Direction::Both if *s == from => Some(*t),
                    Direction::Both if *t == from => Some(*s),
                    _ => None,
                })
            },
            edge_label_fn: &|e| graph.edges.get(&e).map(|(_, _, l)| l.clone()),
        };
        let steps = vec![PatternStep::new(["Post"], Direction::Out, Some("FOLLOWS"), ["User"])];
        let bindings = match_pattern(InternalNodeId(1), &steps, &ctx);
        assert!(bindings.is_empty());
    }
}
