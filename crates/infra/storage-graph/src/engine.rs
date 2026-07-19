//! `GraphEngine` and its `storage_traits::Engine` implementation.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use storage_traits::{
    BoundKind, Engine, EngineStats, Error as TraitError, Result as TraitResult, TxnOptions,
};

use crate::cursor::GraphCursor;
use crate::error::Error;
use crate::format::{
    decode_key, encode_edge_key, encode_edge_property_key, encode_node_key,
    encode_node_property_key, Metadata, WalRecord, EDGE_STORE_FILE, META_FILE, NODE_STORE_FILE,
};
use crate::id::{InternalEdgeId, InternalNodeId};
use crate::index::adjacency::Direction;
use crate::index::GraphIndex;
use crate::model::{Edge, Node, PropertyMap};
use crate::options::GraphOptions;
use crate::query::pattern::PatternContext;
use crate::query::traversal::{TraversalContext, find_path};
use crate::query::{GraphQuery, QueryResult};
use crate::stats::GraphStats;
use crate::store::{EdgeStore, NodeStore};
use crate::transaction::GraphTransaction;
use crate::wal::GraphWal;

/// Inner engine state shared between the public handle and transactions.
pub(crate) struct Inner {
    pub dir: PathBuf,
    pub options: GraphOptions,
    pub metadata: RwLock<Metadata>,
    pub node_store: RwLock<Arc<NodeStore>>,
    pub edge_store: RwLock<Arc<EdgeStore>>,
    pub index: RwLock<GraphIndex>,
    pub wal: GraphWal,
    pub write_lock: Mutex<()>,
    pub unsynced_count: Mutex<usize>,
}

/// A synchronous, durable labeled property graph storage engine.
#[derive(Clone)]
pub struct GraphEngine {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for GraphEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEngine")
            .field("dir", &self.inner.dir)
            .field("options", &self.inner.options)
            .finish()
    }
}

impl GraphEngine {
    /// Open or create a graph engine at `dir` with `options`.
    pub fn open(dir: impl AsRef<Path>, options: GraphOptions) -> crate::Result<Self> {
        options.validate()?;
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let meta_path = dir.join(META_FILE);
        let mut metadata = if meta_path.exists() {
            let bytes = std::fs::read(&meta_path)?;
            Metadata::decode(&bytes)?
        } else {
            Metadata::new(options.clone())
        };
        metadata.options = options.clone();

        let node_store_path = dir.join(NODE_STORE_FILE);
        let edge_store_path = dir.join(EDGE_STORE_FILE);
        let node_store_arc = Arc::new(NodeStore::open(&node_store_path, metadata.node_file_id)?);
        let edge_store_arc = Arc::new(EdgeStore::open(&edge_store_path, metadata.edge_file_id)?);

        let wal = GraphWal::open(&dir, options.wal_sync_policy)?;

        let mut index = crate::recovery::load_or_build_index(
            &dir,
            &node_store_arc,
            &edge_store_arc,
            &metadata,
        )?;
        crate::recovery::replay_wal(&wal, &node_store_arc, &edge_store_arc, &mut index)?;

        let engine = Self {
            inner: Arc::new(Inner {
                dir,
                options,
                metadata: RwLock::new(metadata.clone()),
                node_store: RwLock::new(node_store_arc),
                edge_store: RwLock::new(edge_store_arc),
                index: RwLock::new(index),
                wal,
                write_lock: Mutex::new(()),
                unsynced_count: Mutex::new(0),
            }),
        };
        engine.persist_meta()?;
        Ok(engine)
    }

    pub(crate) fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }

    /// Validate an id length.
    fn check_id_len(&self, id: &[u8]) -> crate::Result<()> {
        if id.len() > self.inner.options.max_key_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Key,
                limit: self.inner.options.max_key_len,
                got: id.len(),
            }
            .into());
        }
        if id.is_empty() {
            return Err(Error::invalid_argument("id must not be empty"));
        }
        Ok(())
    }

    /// Validate a property value length.
    fn check_value_len(&self, value: &[u8]) -> crate::Result<()> {
        if value.len() > self.inner.options.max_value_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Value,
                limit: self.inner.options.max_value_len,
                got: value.len(),
            }
            .into());
        }
        Ok(())
    }

    /// Validate labels.
    fn check_labels(&self, labels: &BTreeSet<String>) -> crate::Result<()> {
        if labels.len() > self.inner.options.max_labels {
            return Err(Error::invalid_argument(format!(
                "too many labels: limit {}, got {}",
                self.inner.options.max_labels,
                labels.len()
            )));
        }
        for label in labels {
            if label.len() > self.inner.options.max_label_len {
                return Err(TraitError::OutOfBounds {
                    kind: BoundKind::Value,
                    limit: self.inner.options.max_label_len,
                    got: label.len(),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Create or replace a node.
    pub fn create_node(
        &self,
        id: impl Into<Vec<u8>>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        let id = id.into();
        self.check_id_len(&id)?;
        let labels: BTreeSet<String> = labels.into_iter().map(Into::into).collect();
        self.check_labels(&labels)?;
        for value in properties.values() {
            self.check_value_len(value)?;
        }
        let _guard = self.inner.write_lock.lock();
        self.create_node_unlocked(id, labels, properties)
    }

    pub(crate) fn create_node_unlocked(
        &self,
        id: Vec<u8>,
        labels: BTreeSet<String>,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        self.inner.wal.append(WalRecord::CreateNode {
            id: id.clone(),
            labels: labels.iter().cloned().collect(),
            properties: properties.clone(),
        })?;
        let node = Node { id, labels, properties };
        let address = self.inner.node_store.read().append(&node)?;
        let mut index = self.inner.index.write();
        index.insert_node(node.id.clone(), address, node.labels.clone());
        drop(index);
        self.bump_unsynced();
        Ok(())
    }

    /// Create or replace an edge.
    pub fn create_edge(
        &self,
        id: impl Into<Vec<u8>>,
        from: impl Into<Vec<u8>>,
        to: impl Into<Vec<u8>>,
        label: impl Into<String>,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        let id = id.into();
        let from = from.into();
        let to = to.into();
        let label = label.into();
        self.check_id_len(&id)?;
        self.check_id_len(&from)?;
        self.check_id_len(&to)?;
        self.check_value_len(label.as_bytes())?;
        for value in properties.values() {
            self.check_value_len(value)?;
        }
        let _guard = self.inner.write_lock.lock();
        self.create_edge_unlocked(id, from, to, label, properties)
    }

    pub(crate) fn create_edge_unlocked(
        &self,
        id: Vec<u8>,
        from: Vec<u8>,
        to: Vec<u8>,
        label: String,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        let index = self.inner.index.read();
        let from_internal = index
            .get_node(&from)
            .map(|(i, _)| i)
            .ok_or_else(|| Error::not_found(format!(
                "source node {} not found",
                String::from_utf8_lossy(&from)
            )))?;
        let to_internal = index
            .get_node(&to)
            .map(|(i, _)| i)
            .ok_or_else(|| Error::not_found(format!(
                "target node {} not found",
                String::from_utf8_lossy(&to)
            )))?;
        drop(index);

        self.inner.wal.append(WalRecord::CreateEdge {
            id: id.clone(),
            from: from.clone(),
            to: to.clone(),
            label: label.clone(),
            properties: properties.clone(),
        })?;
        let edge = Edge {
            id,
            from,
            to,
            label,
            properties,
        };
        let address = self.inner.edge_store.read().append(&edge)?;
        let mut index = self.inner.index.write();
        index.insert_edge(edge.id.clone(), from_internal, to_internal, address, edge.label.clone());
        drop(index);
        self.bump_unsynced();
        Ok(())
    }

    /// Delete a node and all incident edges.
    pub fn delete_node(&self, id: &[u8]) -> crate::Result<bool> {
        self.check_id_len(id)?;
        let _guard = self.inner.write_lock.lock();
        self.delete_node_unlocked(id)
    }

    pub(crate) fn delete_node_unlocked(&self, id: &[u8]) -> crate::Result<bool> {
        let exists = {
            let index = self.inner.index.read();
            index.get_node(id).is_some()
        };
        if !exists {
            return Ok(false);
        }
        self.inner.wal.append(WalRecord::DeleteNode { id: id.to_vec() })?;
        let mut index = self.inner.index.write();
        index.delete_node(id);
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Delete an edge.
    pub fn delete_edge(&self, id: &[u8]) -> crate::Result<bool> {
        self.check_id_len(id)?;
        let _guard = self.inner.write_lock.lock();
        self.delete_edge_unlocked(id)
    }

    pub(crate) fn delete_edge_unlocked(&self, id: &[u8]) -> crate::Result<bool> {
        let exists = {
            let index = self.inner.index.read();
            index.get_edge(id).is_some()
        };
        if !exists {
            return Ok(false);
        }
        self.inner.wal.append(WalRecord::DeleteEdge { id: id.to_vec() })?;
        let mut index = self.inner.index.write();
        index.delete_edge(id);
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Set or overwrite a node property.
    pub fn set_node_property(
        &self,
        id: &[u8],
        key: &str,
        value: Vec<u8>,
    ) -> crate::Result<bool> {
        self.check_id_len(id)?;
        if key.is_empty() {
            return Err(Error::invalid_argument("property key must not be empty"));
        }
        self.check_value_len(&value)?;
        let _guard = self.inner.write_lock.lock();
        self.set_node_property_unlocked(id, key, value)
    }

    pub(crate) fn set_node_property_unlocked(
        &self,
        id: &[u8],
        key: &str,
        value: Vec<u8>,
    ) -> crate::Result<bool> {
        let mut node = match self.get_node(id)? {
            Some(n) => n,
            None => return Ok(false),
        };
        node.properties.insert(key.to_string(), value.clone());
        self.inner.wal.append(WalRecord::SetNodeProperty {
            id: id.to_vec(),
            key: key.to_string(),
            value,
        })?;
        let address = self.inner.node_store.read().append(&node)?;
        let mut index = self.inner.index.write();
        let internal = index.get_node(id).map(|(i, _)| i).ok_or_else(|| {
            Error::corruption("node present in store but missing from index")
        })?;
        index.insert_node(id.to_vec(), address, node.labels);
        index.set_node_property(internal, key);
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Delete a node property.
    pub fn delete_node_property(&self, id: &[u8], key: &str) -> crate::Result<bool> {
        self.check_id_len(id)?;
        if key.is_empty() {
            return Err(Error::invalid_argument("property key must not be empty"));
        }
        let _guard = self.inner.write_lock.lock();
        self.delete_node_property_unlocked(id, key)
    }

    pub(crate) fn delete_node_property_unlocked(
        &self,
        id: &[u8],
        key: &str,
    ) -> crate::Result<bool> {
        let mut node = match self.get_node(id)? {
            Some(n) => n,
            None => return Ok(false),
        };
        if node.properties.remove(key).is_none() {
            return Ok(false);
        }
        self.inner.wal.append(WalRecord::DeleteNodeProperty {
            id: id.to_vec(),
            key: key.to_string(),
        })?;
        let address = self.inner.node_store.read().append(&node)?;
        let mut index = self.inner.index.write();
        let internal = index.get_node(id).map(|(i, _)| i).ok_or_else(|| {
            Error::corruption("node present in store but missing from index")
        })?;
        index.insert_node(id.to_vec(), address, node.labels);
        index.delete_node_property(internal, key);
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Add a label to a node.
    pub fn add_node_label(&self, id: &[u8], label: &str) -> crate::Result<bool> {
        self.check_id_len(id)?;
        if label.is_empty() {
            return Err(Error::invalid_argument("label must not be empty"));
        }
        if label.len() > self.inner.options.max_label_len {
            return Err(TraitError::OutOfBounds {
                kind: BoundKind::Value,
                limit: self.inner.options.max_label_len,
                got: label.len(),
            }
            .into());
        }
        let _guard = self.inner.write_lock.lock();
        self.add_node_label_unlocked(id, label)
    }

    pub(crate) fn add_node_label_unlocked(&self, id: &[u8], label: &str) -> crate::Result<bool> {
        let mut node = match self.get_node(id)? {
            Some(n) => n,
            None => return Ok(false),
        };
        if !node.labels.insert(label.to_string()) {
            return Ok(false);
        }
        self.inner.wal.append(WalRecord::AddNodeLabel {
            id: id.to_vec(),
            label: label.to_string(),
        })?;
        let address = self.inner.node_store.read().append(&node)?;
        let mut index = self.inner.index.write();
        let internal = index.get_node(id).map(|(i, _)| i).ok_or_else(|| {
            Error::corruption("node present in store but missing from index")
        })?;
        index.insert_node(id.to_vec(), address, node.labels);
        index.add_node_label(internal, label.to_string());
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Remove a label from a node.
    pub fn remove_node_label(&self, id: &[u8], label: &str) -> crate::Result<bool> {
        self.check_id_len(id)?;
        if label.is_empty() {
            return Err(Error::invalid_argument("label must not be empty"));
        }
        let _guard = self.inner.write_lock.lock();
        self.remove_node_label_unlocked(id, label)
    }

    pub(crate) fn remove_node_label_unlocked(&self, id: &[u8], label: &str) -> crate::Result<bool> {
        let mut node = match self.get_node(id)? {
            Some(n) => n,
            None => return Ok(false),
        };
        if !node.labels.remove(label) {
            return Ok(false);
        }
        self.inner.wal.append(WalRecord::RemoveNodeLabel {
            id: id.to_vec(),
            label: label.to_string(),
        })?;
        let address = self.inner.node_store.read().append(&node)?;
        let mut index = self.inner.index.write();
        let internal = index.get_node(id).map(|(i, _)| i).ok_or_else(|| {
            Error::corruption("node present in store but missing from index")
        })?;
        index.insert_node(id.to_vec(), address, node.labels);
        index.remove_node_label(internal, label);
        drop(index);
        self.bump_unsynced();
        Ok(true)
    }

    /// Fetch a node by id.
    pub fn get_node(&self, id: &[u8]) -> crate::Result<Option<Node>> {
        let address = {
            let index = self.inner.index.read();
            index.get_node(id).map(|(_, entry)| entry.address)
        };
        match address {
            Some(address) => self.inner.node_store.read().get(address),
            None => Ok(None),
        }
    }

    /// Fetch an edge by id.
    pub fn get_edge(&self, id: &[u8]) -> crate::Result<Option<Edge>> {
        let address = {
            let index = self.inner.index.read();
            index.get_edge(id).map(|(_, entry)| entry.address)
        };
        match address {
            Some(address) => self.inner.edge_store.read().get(address),
            None => Ok(None),
        }
    }

    /// Return neighbor nodes of `id` in `direction`, optionally filtered by
    /// edge label.
    pub fn neighbors(
        &self,
        id: &[u8],
        direction: Direction,
        edge_label: Option<&str>,
    ) -> crate::Result<Vec<Node>> {
        let addresses: Vec<_> = {
            let index = self.inner.index.read();
            let internal = match index.get_node(id) {
                Some((i, _)) => i,
                None => return Ok(Vec::new()),
            };
            let mut out = Vec::new();
            for edge_id in index.edges(internal, direction) {
                let entry = match index.get_edge_entry(edge_id) {
                    Some(e) => e,
                    None => continue,
                };
                if let Some(filter) = edge_label
                    && entry.label != filter
                {
                    continue;
                }
                let neighbor = match direction {
                    Direction::Out => entry.to,
                    Direction::In => entry.from,
                    Direction::Both => {
                        if entry.from == internal {
                            entry.to
                        } else {
                            entry.from
                        }
                    }
                };
                if let Some(entry) = index.get_node_entry(neighbor) {
                    out.push(entry.address);
                }
            }
            out
        };
        let mut nodes = Vec::with_capacity(addresses.len());
        for address in addresses {
            if let Some(node) = self.inner.node_store.read().get(address)? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    /// Return edges incident to `id` in `direction`.
    pub fn edges(&self, id: &[u8], direction: Direction) -> crate::Result<Vec<Edge>> {
        let addresses: Vec<_> = {
            let index = self.inner.index.read();
            let internal = match index.get_node(id) {
                Some((i, _)) => i,
                None => return Ok(Vec::new()),
            };
            index
                .edges(internal, direction)
                .into_iter()
                .filter_map(|edge_id| index.get_edge_entry(edge_id).map(|e| e.address))
                .collect()
        };
        let mut edges = Vec::with_capacity(addresses.len());
        for address in addresses {
            if let Some(edge) = self.inner.edge_store.read().get(address)? {
                edges.push(edge);
            }
        }
        Ok(edges)
    }

    /// Execute a graph query.
    pub fn query(&self, query: GraphQuery) -> crate::Result<QueryResult> {
        match query {
            GraphQuery::NodeById(id) => {
                let node = self.get_node(&id)?;
                Ok(QueryResult::nodes(node.into_iter().collect()))
            }
            GraphQuery::EdgeById(id) => {
                let edge = self.get_edge(&id)?;
                Ok(QueryResult::edges(edge.into_iter().collect()))
            }
            GraphQuery::Neighbors {
                node,
                direction,
                edge_label,
            } => {
                let nodes = self.neighbors(&node, direction, edge_label.as_deref())?;
                Ok(QueryResult::nodes(nodes))
            }
            GraphQuery::NodesByLabel(label) => {
                let index = self.inner.index.read();
                let ids: Vec<_> = index.nodes_with_label(&label).to_vec();
                drop(index);
                let mut nodes = Vec::with_capacity(ids.len());
                for id in ids {
                    let node = self.get_node_by_internal(id)?;
                    if let Some(n) = node {
                        nodes.push(n);
                    }
                }
                Ok(QueryResult::nodes(nodes))
            }
            GraphQuery::EdgesByLabel(label) => {
                let index = self.inner.index.read();
                let ids: Vec<_> = index.edges_with_label(&label).to_vec();
                drop(index);
                let mut edges = Vec::with_capacity(ids.len());
                for id in ids {
                    let edge = self.get_edge_by_internal(id)?;
                    if let Some(e) = edge {
                        edges.push(e);
                    }
                }
                Ok(QueryResult::edges(edges))
            }
            GraphQuery::Path { from, to, max_depth } => {
                let path_ids: Option<Vec<InternalNodeId>> = {
                    let index = self.inner.index.read();
                    let from_internal = match index.get_node(&from) {
                        Some((i, _)) => i,
                        None => return Ok(QueryResult::new()),
                    };
                    let to_internal = match index.get_node(&to) {
                        Some((i, _)) => i,
                        None => return Ok(QueryResult::new()),
                    };
                    let ctx = TraversalContext {
                        current: InternalNodeId(0),
                        depth: 0,
                        edges_fn: &|node, direction| index.edges(node, direction),
                        target_fn: &|from_id, edge_id| {
                            index.get_edge_entry(edge_id).and_then(|entry| {
                                if entry.from == from_id {
                                    Some(entry.to)
                                } else if entry.to == from_id {
                                    Some(entry.from)
                                } else {
                                    None
                                }
                            })
                        },
                        edge_label: None,
                        label_fn: &|edge_id| {
                            index.get_edge_entry(edge_id).map(|e| e.label.clone())
                        },
                    };
                    find_path(from_internal, to_internal, max_depth, Direction::Out, &ctx)
                };
                match path_ids {
                    Some(ids) => {
                        let mut path = Vec::with_capacity(ids.len());
                        for id in ids {
                            let node = self.get_node_by_internal(id)?;
                            if let Some(n) = node {
                                path.push(n.id);
                            }
                        }
                        Ok(QueryResult::paths(vec![path]))
                    }
                    None => Ok(QueryResult::new()),
                }
            }
            GraphQuery::Pattern(steps) => {
                let bindings: Vec<Vec<InternalNodeId>> = {
                    let index = self.inner.index.read();
                    let starts: Vec<_> = if steps.is_empty() || steps[0].node_labels.is_empty() {
                        index.iter_nodes().map(|(i, _)| *i).collect()
                    } else {
                        let mut candidates: HashSet<InternalNodeId> = HashSet::new();
                        for label in &steps[0].node_labels {
                            for id in index.nodes_with_label(label) {
                                candidates.insert(*id);
                            }
                        }
                        candidates.into_iter().collect()
                    };
                    let ctx = PatternContext {
                        node_labels_fn: &|id| {
                            index.get_node_entry(id).map(|entry| entry.labels.clone())
                        },
                        edges_fn: &|node, direction| index.edges(node, direction),
                        endpoint_fn: &|from_id, edge_id, direction| {
                            index.get_edge_entry(edge_id).and_then(|entry| match direction {
                                Direction::Out if entry.from == from_id => Some(entry.to),
                                Direction::In if entry.to == from_id => Some(entry.from),
                                Direction::Both if entry.from == from_id => Some(entry.to),
                                Direction::Both if entry.to == from_id => Some(entry.from),
                                _ => None,
                            })
                        },
                        edge_label_fn: &|edge_id| {
                            index.get_edge_entry(edge_id).map(|e| e.label.clone())
                        },
                    };
                    let mut all = Vec::new();
                    for start in starts {
                        for binding in crate::query::pattern::match_pattern(start, &steps, &ctx) {
                            all.push(binding);
                        }
                    }
                    all
                };
                let mut paths: Vec<Vec<Vec<u8>>> = Vec::with_capacity(bindings.len());
                for binding in bindings {
                    let mut path = Vec::with_capacity(binding.len());
                    for id in binding {
                        let node = self.get_node_by_internal(id)?;
                        if let Some(n) = node {
                            path.push(n.id);
                        }
                    }
                    paths.push(path);
                }
                Ok(QueryResult::paths(paths))
            }
        }
    }

    fn get_node_by_internal(&self, internal: InternalNodeId) -> crate::Result<Option<Node>> {
        let address = {
            let index = self.inner.index.read();
            index.get_node_entry(internal).map(|e| e.address)
        };
        match address {
            Some(addr) => self.inner.node_store.read().get(addr),
            None => Ok(None),
        }
    }

    fn get_edge_by_internal(&self, internal: InternalEdgeId) -> crate::Result<Option<Edge>> {
        let address = {
            let index = self.inner.index.read();
            index.get_edge_entry(internal).map(|e| e.address)
        };
        match address {
            Some(addr) => self.inner.edge_store.read().get(addr),
            None => Ok(None),
        }
    }

    fn bump_unsynced(&self) {
        let mut count = self.inner.unsynced_count.lock();
        *count += 1;
    }

    /// Flush stores, persist indexes, write metadata, and checkpoint WAL.
    pub fn sync(&self) -> crate::Result<()> {
        let _guard = self.inner.write_lock.lock();
        self.inner.node_store.read().sync()?;
        self.inner.edge_store.read().sync()?;
        self.maybe_compact()?;
        self.persist_index()?;
        self.persist_meta_with_checkpoint()?;
        self.inner.wal.sync()?;
        self.inner.wal.truncate_completed()?;
        *self.inner.unsynced_count.lock() = 0;
        Ok(())
    }

    fn maybe_compact(&self) -> crate::Result<()> {
        let should_compact = {
            let index = self.inner.index.read();
            index.deletion_ratio() > self.inner.options.compaction_threshold
        };
        if should_compact {
            crate::compaction::compact(self)?;
        }
        Ok(())
    }

    /// Persist the current index snapshot to disk.
    pub fn persist_index(&self) -> crate::Result<()> {
        let index = self.inner.index.read();
        let bytes = bincode::serialize(&*index).map_err(|e| crate::Error::corruption(e.to_string()))?;
        let mut buf = Vec::with_capacity(bytes.len() + 4);
        buf.extend_from_slice(&bytes);
        let crc = storage_format::crc32c(&bytes);
        buf.extend_from_slice(&crc.to_le_bytes());
        storage_file::atomic_write(&self.inner.dir.join(crate::format::INDEX_FILE), &buf)?;
        Ok(())
    }

    /// Persist metadata and write a WAL checkpoint.
    fn persist_meta_with_checkpoint(&self) -> crate::Result<()> {
        let mut meta = self.inner.metadata.write();
        let lsn = self.inner.wal.checkpoint(&meta)?;
        meta.wal_checkpoint_lsn = lsn;
        let index = self.inner.index.read();
        meta.next_node_id = index.next_node_id();
        meta.next_edge_id = index.next_edge_id();
        drop(index);
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
        Ok(())
    }

    /// Persist the current metadata file atomically.
    pub fn persist_meta(&self) -> crate::Result<()> {
        let meta = self.inner.metadata.read();
        let encoded = meta.encode()?;
        storage_file::atomic_write(&self.inner.dir.join(META_FILE), &encoded)?;
        Ok(())
    }

    /// Close the engine gracefully.
    pub fn close(&self) -> crate::Result<()> {
        self.sync()?;
        self.inner.wal.close()?;
        Ok(())
    }

    /// Return engine statistics.
    pub fn stats(&self) -> crate::Result<GraphStats> {
        let index = self.inner.index.read();
        let num_nodes = index.node_count() as u64;
        let num_edges = index.edge_count() as u64;
        let memory_bytes = (num_nodes + num_edges) * 64;
        Ok(GraphStats {
            name: "storage-graph",
            num_nodes,
            num_edges,
            disk_bytes: approx_dir_bytes(&self.inner.dir)?,
            memory_bytes,
            metrics: {
                let mut m = std::collections::HashMap::new();
                m.insert("max_key_len".into(), self.inner.options.max_key_len as u64);
                m.insert("deleted_nodes".into(), index.deleted_nodes() as u64);
                m.insert("deleted_edges".into(), index.deleted_edges() as u64);
                m
            },
        })
    }
}

fn approx_dir_bytes(dir: &Path) -> crate::Result<u64> {
    let mut total = 0u64;
    if let Ok(entries) = walkdir(dir) {
        for entry in entries {
            if let Ok(md) = entry.metadata() {
                total += md.len();
            }
        }
    }
    Ok(total)
}

fn walkdir(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    fn collect(path: &Path, out: &mut Vec<std::fs::DirEntry>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(&entry.path(), out)?;
            } else {
                out.push(entry);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    collect(path, &mut out)?;
    Ok(out)
}

impl Engine for GraphEngine {
    type Error = Error;
    type Transaction = GraphTransaction;
    type Cursor = GraphCursor;

    fn name(&self) -> &'static str {
        "storage-graph"
    }

    fn begin(&self, opts: TxnOptions) -> TraitResult<Self::Transaction, Self::Error> {
        Ok(GraphTransaction::new(self.clone(), opts))
    }

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        let (kind, id, prop) = decode_key(key)?;
        match (kind, prop) {
            ("node", None) => {
                let node = self.get_node(id)?;
                Ok(match node {
                    Some(n) => Some(Bytes::from(encode_node_value(&n)?)),
                    None => None,
                })
            }
            ("edge", None) => {
                let edge = self.get_edge(id)?;
                Ok(match edge {
                    Some(e) => Some(Bytes::from(encode_edge_value(&e)?)),
                    None => None,
                })
            }
            ("node", Some(property_key)) => Ok(self
                .get_node(id)?
                .and_then(|n| n.properties.get(property_key).cloned())
                .map(Bytes::from)),
            ("edge", Some(property_key)) => Ok(self
                .get_edge(id)?
                .and_then(|e| e.properties.get(property_key).cloned())
                .map(Bytes::from)),
            _ => Err(Error::corruption("unknown composite key kind")),
        }
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<Self::Cursor, Self::Error> {
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let index = self.inner.index.read();
        for (_, entry) in index.iter_nodes() {
            let key = encode_node_key(&entry.id);
            if in_range(&key, start, end)
                && let Some(node) = self.inner.node_store.read().get(entry.address)?
            {
                map.insert(key, encode_node_value(&node)?);
            }
            if let Ok(Some(node)) = self.inner.node_store.read().get(entry.address) {
                for (prop_key, value) in &node.properties {
                    let key = encode_node_property_key(&entry.id, prop_key);
                    if in_range(&key, start, end) {
                        map.insert(key, value.clone());
                    }
                }
            }
        }
        for (_, entry) in index.iter_edges() {
            let key = encode_edge_key(&entry.id);
            if in_range(&key, start, end)
                && let Some(edge) = self.inner.edge_store.read().get(entry.address)?
            {
                map.insert(key, encode_edge_value(&edge)?);
            }
            if let Ok(Some(edge)) = self.inner.edge_store.read().get(entry.address) {
                for (prop_key, value) in &edge.properties {
                    let key = encode_edge_property_key(&entry.id, prop_key);
                    if in_range(&key, start, end) {
                        map.insert(key, value.clone());
                    }
                }
            }
        }
        drop(index);
        Ok(GraphCursor::new(map))
    }

    fn stats(&self) -> TraitResult<EngineStats, Self::Error> {
        let s = self.stats()?;
        Ok(s.into_engine_stats())
    }

    fn sync(&self) -> TraitResult<(), Self::Error> {
        GraphEngine::sync(self)
    }
}

fn in_range(key: &[u8], start: Option<&[u8]>, end: Option<&[u8]>) -> bool {
    let above_start = start.map(|s| key >= s).unwrap_or(true);
    let below_end = end.map(|e| key < e).unwrap_or(true);
    above_start && below_end
}

fn encode_node_value(node: &Node) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(node).map_err(|e| Error::property_encoding(e.to_string()))
}

fn encode_edge_value(edge: &Edge) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(edge).map_err(|e| Error::property_encoding(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_get_node() {
        let dir = tempfile::tempdir().unwrap();
        let engine = GraphEngine::open(dir.path(), GraphOptions::default()).unwrap();
        engine
            .create_node(b"n1", ["User"], PropertyMap::new())
            .unwrap();
        let node = engine.get_node(b"n1").unwrap().unwrap();
        assert_eq!(node.id, b"n1");
        assert!(node.labels.contains("User"));
    }

    #[test]
    fn create_edge_requires_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let engine = GraphEngine::open(dir.path(), GraphOptions::default()).unwrap();
        let result = engine.create_edge(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn delete_node_cascades() {
        let dir = tempfile::tempdir().unwrap();
        let engine = GraphEngine::open(dir.path(), GraphOptions::default()).unwrap();
        engine
            .create_node(b"n1", ["User"], PropertyMap::new())
            .unwrap();
        engine
            .create_node(b"n2", ["User"], PropertyMap::new())
            .unwrap();
        engine
            .create_edge(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new())
            .unwrap();
        engine.delete_node(b"n1").unwrap();
        assert!(engine.get_edge(b"e1").unwrap().is_none());
    }
}
