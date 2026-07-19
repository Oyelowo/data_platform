//! Graph traversal (BFS/DFS) with optional edge-label filters.

use std::collections::{HashSet, VecDeque};

use crate::id::{InternalEdgeId, InternalNodeId};
use crate::index::adjacency::Direction;

/// Traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Breadth-first search.
    BreadthFirst,
    /// Depth-first search.
    DepthFirst,
}

/// Context passed to traversal neighbor expansion.
pub struct TraversalContext<'a> {
    /// Internal id of the current node.
    pub current: InternalNodeId,
    /// Current depth from the start node.
    pub depth: usize,
    /// Resolve incident edges for the current node in a direction.
    pub edges_fn: &'a dyn Fn(InternalNodeId, Direction) -> Vec<InternalEdgeId>,
    /// Resolve the target node of an edge given its source and edge id.
    pub target_fn: &'a dyn Fn(InternalNodeId, InternalEdgeId) -> Option<InternalNodeId>,
    /// Optional label filter applied to edges.
    pub edge_label: Option<&'a str>,
    /// Resolve the label of an edge.
    pub label_fn: &'a dyn Fn(InternalEdgeId) -> Option<String>,
}

impl<'a> TraversalContext<'a> {
    /// Return neighbor node ids reachable from `node` in `direction` that pass
    /// the optional edge-label filter.
    pub fn neighbors(&self, node: InternalNodeId, direction: Direction) -> Vec<InternalNodeId> {
        let mut out = Vec::new();
        for edge_id in (self.edges_fn)(node, direction) {
            if let Some(label) = (self.label_fn)(edge_id)
                && let Some(filter) = self.edge_label
                && label != filter
            {
                continue;
            }
            if let Some(target) = (self.target_fn)(node, edge_id) {
                out.push(target);
            }
        }
        out
    }
}

/// Run a traversal from `start` up to `max_depth`.
///
/// Returns the visited node ids in discovery order.
pub fn traverse(
    start: InternalNodeId,
    max_depth: usize,
    order: Order,
    direction: Direction,
    ctx: &TraversalContext<'_>,
) -> Vec<InternalNodeId> {
    let mut visited: HashSet<InternalNodeId> = HashSet::new();
    let mut result = Vec::new();
    match order {
        Order::BreadthFirst => {
            let mut queue = VecDeque::new();
            queue.push_back((start, 0usize));
            visited.insert(start);
            result.push(start);
            while let Some((node, depth)) = queue.pop_front() {
                if depth >= max_depth {
                    continue;
                }
                for neighbor in ctx.neighbors(node, direction) {
                    if visited.insert(neighbor) {
                        result.push(neighbor);
                        queue.push_back((neighbor, depth + 1));
                    }
                }
            }
        }
        Order::DepthFirst => {
            let mut stack = vec![(start, 0usize)];
            while let Some((node, depth)) = stack.pop() {
                if visited.insert(node) {
                    result.push(node);
                    if depth < max_depth {
                        for neighbor in ctx.neighbors(node, direction) {
                            if !visited.contains(&neighbor) {
                                stack.push((neighbor, depth + 1));
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

/// Find any path from `from` to `to` with a maximum length of `max_depth`
/// edges using BFS.
pub fn find_path(
    from: InternalNodeId,
    to: InternalNodeId,
    max_depth: usize,
    direction: Direction,
    ctx: &TraversalContext<'_>,
) -> Option<Vec<InternalNodeId>> {
    if from == to {
        return Some(vec![from]);
    }
    let mut visited: HashSet<InternalNodeId> = HashSet::new();
    let mut queue: VecDeque<(InternalNodeId, Vec<InternalNodeId>)> = VecDeque::new();
    visited.insert(from);
    queue.push_back((from, vec![from]));
    while let Some((node, path)) = queue.pop_front() {
        let depth = path.len() - 1;
        if depth >= max_depth {
            continue;
        }
        for neighbor in ctx.neighbors(node, direction) {
            if visited.insert(neighbor) {
                let mut next_path = path.clone();
                next_path.push(neighbor);
                if neighbor == to {
                    return Some(next_path);
                }
                queue.push_back((neighbor, next_path));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Graph {
        edges: BTreeMap<InternalEdgeId, (InternalNodeId, InternalNodeId, String)>,
        adjacency: BTreeMap<InternalNodeId, Vec<InternalEdgeId>>,
    }

    fn g() -> Graph {
        let mut edges: BTreeMap<InternalEdgeId, (InternalNodeId, InternalNodeId, String)> = BTreeMap::new();
        let mut adjacency: BTreeMap<InternalNodeId, Vec<InternalEdgeId>> = BTreeMap::new();
        let e = |i| InternalEdgeId(i);
        let n = |i| InternalNodeId(i);
        edges.insert(e(1), (n(1), n(2), "A".into()));
        edges.insert(e(2), (n(2), n(3), "A".into()));
        edges.insert(e(3), (n(1), n(3), "B".into()));
        adjacency.entry(n(1)).or_default().extend([e(1), e(3)]);
        adjacency.entry(n(2)).or_default().push(e(2));
        Graph { edges, adjacency }
    }

    #[test]
    fn bfs_ordering() {
        let graph = g();
        let ctx = TraversalContext {
            current: InternalNodeId(0),
            depth: 0,
            edges_fn: &|node, _direction| {
                graph.adjacency.get(&node).cloned().unwrap_or_default()
            },
            target_fn: &|from, edge_id| {
                graph.edges.get(&edge_id).and_then(|(s, t, _)| {
                    if *s == from { Some(*t) } else { None }
                })
            },
            edge_label: None,
            label_fn: &|edge_id| graph.edges.get(&edge_id).map(|(_, _, l)| l.clone()),
        };
        let visited = traverse(
            InternalNodeId(1),
            10,
            Order::BreadthFirst,
            Direction::Out,
            &ctx,
        );
        assert_eq!(visited, vec![InternalNodeId(1), InternalNodeId(2), InternalNodeId(3)]);
    }

    #[test]
    fn dfs_ordering() {
        let graph = g();
        let ctx = TraversalContext {
            current: InternalNodeId(0),
            depth: 0,
            edges_fn: &|node, _direction| {
                graph.adjacency.get(&node).cloned().unwrap_or_default()
            },
            target_fn: &|from, edge_id| {
                graph.edges.get(&edge_id).and_then(|(s, t, _)| {
                    if *s == from { Some(*t) } else { None }
                })
            },
            edge_label: None,
            label_fn: &|edge_id| graph.edges.get(&edge_id).map(|(_, _, l)| l.clone()),
        };
        let visited = traverse(
            InternalNodeId(1),
            10,
            Order::DepthFirst,
            Direction::Out,
            &ctx,
        );
        assert_eq!(visited[0], InternalNodeId(1));
        assert_eq!(visited.len(), 3);
    }

    #[test]
    fn label_filter() {
        let graph = g();
        let ctx = TraversalContext {
            current: InternalNodeId(0),
            depth: 0,
            edges_fn: &|node, _direction| {
                graph.adjacency.get(&node).cloned().unwrap_or_default()
            },
            target_fn: &|from, edge_id| {
                graph.edges.get(&edge_id).and_then(|(s, t, _)| {
                    if *s == from { Some(*t) } else { None }
                })
            },
            edge_label: Some("A"),
            label_fn: &|edge_id| graph.edges.get(&edge_id).map(|(_, _, l)| l.clone()),
        };
        let visited = traverse(
            InternalNodeId(1),
            10,
            Order::BreadthFirst,
            Direction::Out,
            &ctx,
        );
        assert_eq!(visited, vec![InternalNodeId(1), InternalNodeId(2), InternalNodeId(3)]);
    }

    #[test]
    fn find_path_basic() {
        let graph = g();
        let ctx = TraversalContext {
            current: InternalNodeId(0),
            depth: 0,
            edges_fn: &|node, _direction| {
                graph.adjacency.get(&node).cloned().unwrap_or_default()
            },
            target_fn: &|from, edge_id| {
                graph.edges.get(&edge_id).and_then(|(s, t, _)| {
                    if *s == from { Some(*t) } else { None }
                })
            },
            edge_label: None,
            label_fn: &|edge_id| graph.edges.get(&edge_id).map(|(_, _, l)| l.clone()),
        };
        let path = find_path(
            InternalNodeId(1),
            InternalNodeId(3),
            10,
            Direction::Out,
            &ctx,
        );
        assert!(path.is_some());
    }
}
