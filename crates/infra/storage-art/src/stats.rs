//! Depth and node-count metrics for `ArtMap`.

use std::collections::HashMap;

use storage_traits::EngineStats;

use crate::map::ArtMap;
use crate::node::{NodeType, ptr_to_arc};

/// Compute engine statistics for an `ArtMap`.
pub fn engine_stats(map: &ArtMap) -> EngineStats {
    let mut metrics = HashMap::new();
    let (node_count, max_depth) = walk_tree(map);
    metrics.insert("node_count".to_string(), node_count);
    metrics.insert("max_depth".to_string(), max_depth);
    EngineStats {
        name: "storage-art",
        disk_bytes: 0,
        memory_bytes: 0,
        num_keys: Some(map.len() as u64),
        metrics,
    }
}

/// Return `(node_count, max_depth)` for the tree.
fn walk_tree(map: &ArtMap) -> (u64, u64) {
    let root_ptr = map.root_ptr();
    if root_ptr.is_null() {
        return (0, 0);
    }
    let root = match unsafe { ptr_to_arc(root_ptr) } {
        Some(r) => r,
        None => return (0, 0),
    };
    let mut count = 0u64;
    let mut max_depth = 0u64;
    let mut stack = vec![(root, 1u64)];
    while let Some((node, depth)) = stack.pop() {
        count += 1;
        max_depth = max_depth.max(depth);
        if node.node_type() != NodeType::Leaf {
            let mut next_byte: Option<u8> = None;
            loop {
                let child_info = match next_byte {
                    None => node.first_child(),
                    Some(b) => node.next_child(b),
                };
                match child_info {
                    Some((byte, ptr)) if !ptr.is_null() => {
                        if let Some(child) = unsafe { ptr_to_arc(ptr) } {
                            next_byte = Some(byte);
                            stack.push((child, depth + 1));
                        }
                    }
                    _ => break,
                }
            }
        }
    }
    (count, max_depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ArtMapOptions;

    #[test]
    fn stats_empty() {
        let map = ArtMap::new(ArtMapOptions::default());
        let stats = engine_stats(&map);
        assert_eq!(stats.num_keys, Some(0));
        assert_eq!(stats.metrics.get("node_count"), Some(&0));
    }
}
