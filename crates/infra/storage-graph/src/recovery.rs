//! Recovery: index rebuild and WAL replay for `storage-graph`.

use std::path::Path;

use crate::format::{Metadata, WalRecord, INDEX_FILE};
use crate::index::GraphIndex;
use crate::model::{Edge, Node};
use crate::store::{EdgeStore, NodeStore};
use crate::wal::GraphWal;

/// Load a persisted index snapshot or rebuild it from the stores.
pub fn load_or_build_index(
    dir: &Path,
    node_store: &NodeStore,
    edge_store: &EdgeStore,
    metadata: &Metadata,
) -> crate::Result<GraphIndex> {
    let index_path = dir.join(INDEX_FILE);
    if index_path.exists() {
        let bytes = std::fs::read(&index_path)?;
        if let Some(index) = decode_index_snapshot(&bytes)? {
            return Ok(index);
        }
    }
    build_index_from_stores(node_store, edge_store, metadata)
}

fn decode_index_snapshot(bytes: &[u8]) -> crate::Result<Option<GraphIndex>> {
    if bytes.len() < 4 {
        return Ok(None);
    }
    let (body, crc_bytes) = bytes.split_at(bytes.len() - 4);
    let stored_crc = u32::from_le_bytes(
        crc_bytes
            .try_into()
            .map_err(|_| crate::Error::corruption("index checksum truncated"))?,
    );
    let computed_crc = storage_format::crc32c(body);
    if stored_crc != computed_crc {
        return Ok(None);
    }
    bincode::deserialize(body)
        .map(Some)
        .map_err(|e| crate::Error::corruption(e.to_string()))
}

/// Scan the node and edge stores to reconstruct the index.
pub fn build_index_from_stores(
    node_store: &NodeStore,
    edge_store: &EdgeStore,
    _metadata: &Metadata,
) -> crate::Result<GraphIndex> {
    let mut index = GraphIndex::new();

    for result in node_store.iter()? {
        let (address, node) = result?;
        index.insert_node(node.id, address, node.labels);
    }
    for result in edge_store.iter()? {
        let (address, edge) = result?;
        let from_internal = match index.get_node(&edge.from) {
            Some((id, _)) => id,
            None => {
                return Err(crate::Error::corruption(format!(
                    "edge {} references missing source node {}",
                    String::from_utf8_lossy(&edge.id),
                    String::from_utf8_lossy(&edge.from)
                )))
            }
        };
        let to_internal = match index.get_node(&edge.to) {
            Some((id, _)) => id,
            None => {
                return Err(crate::Error::corruption(format!(
                    "edge {} references missing target node {}",
                    String::from_utf8_lossy(&edge.id),
                    String::from_utf8_lossy(&edge.to)
                )))
            }
        };
        index.insert_edge(edge.id, from_internal, to_internal, address, edge.label);
    }
    Ok(index)
}

/// Replay all WAL records on top of the current index and stores.
pub fn replay_wal(
    wal: &GraphWal,
    node_store: &NodeStore,
    edge_store: &EdgeStore,
    index: &mut GraphIndex,
) -> crate::Result<()> {
    for result in wal.iter()? {
        let (_lsn, record) = result?;
        apply_wal_record(index, node_store, edge_store, record)?;
    }
    Ok(())
}

fn apply_wal_record(
    index: &mut GraphIndex,
    node_store: &NodeStore,
    edge_store: &EdgeStore,
    record: WalRecord,
) -> crate::Result<()> {
    match record {
        WalRecord::CreateNode { id, labels, properties } => {
            let node = Node {
                id: id.clone(),
                labels: labels.into_iter().collect(),
                properties,
            };
            let address = node_store.append(&node)?;
            index.insert_node(id, address, node.labels);
        }
        WalRecord::DeleteNode { id } => {
            let _ = index.delete_node(&id);
        }
        WalRecord::CreateEdge {
            id,
            from,
            to,
            label,
            properties,
        } => {
            let from_internal = match index.get_node(&from) {
                Some((i, _)) => i,
                None => {
                    // Edge references a missing node; it may have been deleted
                    // after this record was written. Skip it.
                    return Ok(());
                }
            };
            let to_internal = match index.get_node(&to) {
                Some((i, _)) => i,
                None => return Ok(()),
            };
            let edge = Edge {
                id: id.clone(),
                from,
                to,
                label,
                properties,
            };
            let address = edge_store.append(&edge)?;
            index.insert_edge(id, from_internal, to_internal, address, edge.label);
        }
        WalRecord::DeleteEdge { id } => {
            let _ = index.delete_edge(&id);
        }
        WalRecord::SetNodeProperty { id, key, value } => {
            update_node(index, node_store, &id, |node| {
                node.properties.insert(key, value);
            })?;
        }
        WalRecord::DeleteNodeProperty { id, key } => {
            update_node(index, node_store, &id, |node| {
                node.properties.remove(&key);
            })?;
        }
        WalRecord::SetEdgeProperty { id, key, value } => {
            update_edge(index, edge_store, &id, |edge| {
                edge.properties.insert(key, value);
            })?;
        }
        WalRecord::DeleteEdgeProperty { id, key } => {
            update_edge(index, edge_store, &id, |edge| {
                edge.properties.remove(&key);
            })?;
        }
        WalRecord::AddNodeLabel { id, label } => {
            update_node(index, node_store, &id, |node| {
                node.labels.insert(label);
            })?;
        }
        WalRecord::RemoveNodeLabel { id, label } => {
            update_node(index, node_store, &id, |node| {
                node.labels.remove(&label);
            })?;
        }
    }
    Ok(())
}

fn update_node<F>(
    index: &mut GraphIndex,
    node_store: &NodeStore,
    id: &[u8],
    f: F,
) -> crate::Result<()>
where
    F: FnOnce(&mut Node),
{
    let (internal, address) = match index.get_node(id) {
        Some((i, entry)) => (i, entry.address),
        None => return Ok(()),
    };
    let mut node = match node_store.get(address)? {
        Some(n) => n,
        None => return Ok(()),
    };
    f(&mut node);
    let new_address = node_store.append(&node)?;
    index.insert_node(id.to_vec(), new_address, node.labels.clone());
    index.set_node_property(internal, "");
    let _ = internal;
    Ok(())
}

fn update_edge<F>(
    index: &mut GraphIndex,
    edge_store: &EdgeStore,
    id: &[u8],
    f: F,
) -> crate::Result<()>
where
    F: FnOnce(&mut Edge),
{
    let (internal, address, from_internal, to_internal) = match index.get_edge(id) {
        Some((i, entry)) => (i, entry.address, entry.from, entry.to),
        None => return Ok(()),
    };
    let mut edge = match edge_store.get(address)? {
        Some(e) => e,
        None => return Ok(()),
    };
    f(&mut edge);
    let new_address = edge_store.append(&edge)?;
    index.insert_edge(
        id.to_vec(),
        from_internal,
        to_internal,
        new_address,
        edge.label.clone(),
    );
    index.set_edge_property(internal, "");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::NODE_STORE_FILE;
    use crate::options::GraphOptions;

    #[test]
    fn build_index_from_empty_stores() {
        let dir = tempfile::tempdir().unwrap();
        let meta = Metadata::new(GraphOptions::default());
        let node_store = NodeStore::open(dir.path().join(NODE_STORE_FILE), 0).unwrap();
        let edge_store = EdgeStore::open(dir.path().join("EDGES"), 0).unwrap();
        let index = build_index_from_stores(&node_store, &edge_store, &meta).unwrap();
        assert_eq!(index.node_count(), 0);
        assert_eq!(index.edge_count(), 0);
    }
}
