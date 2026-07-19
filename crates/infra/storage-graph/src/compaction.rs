//! Compaction rewrites node/edge stores to reclaim deleted-record space.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::engine::GraphEngine;
use crate::format::{EDGE_STORE_FILE, NODE_STORE_FILE};
use crate::index::GraphIndex;
use crate::store::{EdgeStore, NodeStore};

/// Rewrite the node and edge stores, keeping only live records.
pub fn compact(engine: &GraphEngine) -> crate::Result<()> {
    let dir = engine.inner.dir.clone();
    let (new_node_file_id, new_edge_file_id) = {
        let meta = engine.inner.metadata.read();
        (meta.node_file_id + 1, meta.edge_file_id + 1)
    };

    let new_node_path = store_path(&dir, NODE_STORE_FILE, new_node_file_id);
    let new_edge_path = store_path(&dir, EDGE_STORE_FILE, new_edge_file_id);

    let new_node_store = Arc::new(NodeStore::open(&new_node_path, new_node_file_id)?);
    let new_edge_store = Arc::new(EdgeStore::open(&new_edge_path, new_edge_file_id)?);

    let mut new_index = GraphIndex::new();

    // Copy live nodes and update their addresses.
    {
        let index = engine.inner.index.read();
        for (_, entry) in index.iter_nodes() {
            if let Some(node) = engine.inner.node_store.read().get(entry.address)? {
                let address = new_node_store.append(&node)?;
                new_index.insert_node(entry.id.clone(), address, entry.labels.clone());
            }
        }
        // Copy live edges and update their addresses.
        for (_, entry) in index.iter_edges() {
            if let Some(edge) = engine.inner.edge_store.read().get(entry.address)? {
                let from_internal = new_index
                    .get_node(&edge.from)
                    .map(|(i, _)| i)
                    .ok_or_else(|| {
                        crate::Error::corruption(format!(
                            "compaction: edge {} source node {} missing",
                            String::from_utf8_lossy(&edge.id),
                            String::from_utf8_lossy(&edge.from)
                        ))
                    })?;
                let to_internal = new_index
                    .get_node(&edge.to)
                    .map(|(i, _)| i)
                    .ok_or_else(|| {
                        crate::Error::corruption(format!(
                            "compaction: edge {} target node {} missing",
                            String::from_utf8_lossy(&edge.id),
                            String::from_utf8_lossy(&edge.to)
                        ))
                    })?;
                let address = new_edge_store.append(&edge)?;
                new_index.insert_edge(
                    entry.id.clone(),
                    from_internal,
                    to_internal,
                    address,
                    entry.label.clone(),
                );
            }
        }
    }

    new_node_store.sync()?;
    new_edge_store.sync()?;

    // Replace stores and index atomically under the write lock.
    *engine.inner.node_store.write() = new_node_store;
    *engine.inner.edge_store.write() = new_edge_store;
    *engine.inner.index.write() = new_index;

    {
        let mut meta = engine.inner.metadata.write();
        meta.node_file_id = new_node_file_id;
        meta.edge_file_id = new_edge_file_id;
    }

    // Best-effort removal of old store files.
    let old_node_path = store_path(&dir, NODE_STORE_FILE, new_node_file_id - 1);
    let old_edge_path = store_path(&dir, EDGE_STORE_FILE, new_edge_file_id - 1);
    let _ = std::fs::remove_file(&old_node_path);
    let _ = std::fs::remove_file(&old_edge_path);

    Ok(())
}

fn store_path(dir: &Path, base: &str, file_id: u64) -> PathBuf {
    dir.join(format!("{}.{}", base, file_id))
}
