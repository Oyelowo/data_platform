//! Transactions for the graph storage engine.

use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use storage_traits::{Cursor, Engine, IsolationLevel, Result as TraitResult, Transaction, TxnOptions};

use crate::cursor::GraphCursor;
use crate::engine::GraphEngine;
use crate::error::Error;
use crate::format::{decode_key, encode_edge_key, encode_node_key};
use crate::model::{Edge, Node, PropertyMap};

/// A transaction over a [`GraphEngine`](crate::engine::GraphEngine).
pub struct GraphTransaction {
    engine: GraphEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local_puts: BTreeMap<Vec<u8>, Vec<u8>>,
    local_deletes: HashSet<Vec<u8>>,
}

impl GraphTransaction {
    pub(crate) fn new(engine: GraphEngine, opts: TxnOptions) -> Self {
        Self {
            engine,
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local_puts: BTreeMap::new(),
            local_deletes: HashSet::new(),
        }
    }

    fn ensure_active(&self) -> crate::Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for GraphTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        self.ensure_active()?;
        if self.local_deletes.contains(key) {
            return Ok(None);
        }
        if let Some(value) = self.local_puts.get(key) {
            return Ok(Some(Bytes::from(value.clone())));
        }
        self.engine.get(key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_puts.insert(key.to_vec(), value.to_vec());
        self.local_deletes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(key.to_vec());
        self.local_puts.remove(key);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<impl Cursor<Error = Self::Error>, Self::Error> {
        self.ensure_active()?;
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        for item in self.engine.scan(start, end)? {
            let (k, v) = item?;
            map.insert(k.to_vec(), v.to_vec());
        }

        for key in &self.local_deletes {
            map.remove(key);
        }

        for (key, value) in &self.local_puts {
            let in_range = {
                let above_start = start.map(|s| key.as_slice() >= s).unwrap_or(true);
                let below_end = end.map(|e| key.as_slice() < e).unwrap_or(true);
                above_start && below_end
            };
            if in_range {
                map.insert(key.clone(), value.clone());
            }
        }

        Ok(GraphCursor::new(map))
    }

    fn commit(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        let _guard = self.engine.inner().write_lock.lock();

        for key in self.local_deletes {
            let (kind, id, prop) = decode_key(&key)?;
            match (kind, prop) {
                ("node", None) => {
                    self.engine.delete_node_unlocked(id)?;
                }
                ("edge", None) => {
                    self.engine.delete_edge_unlocked(id)?;
                }
                ("node", Some(property_key)) => {
                    self.engine.delete_node_property_unlocked(id, property_key)?;
                }
                ("edge", Some(property_key)) => {
                    let _ = property_key;
                }
                _ => {}
            }
        }

        for (key, value) in self.local_puts {
            let (kind, id, prop) = decode_key(&key)?;
            match (kind, prop) {
                ("node", None) => {
                    let node = decode_node_value(value)?;
                    if node.id != id {
                        return Err(Error::invalid_argument(
                            "node id in key does not match encoded value",
                        ));
                    }
                    self.engine.create_node_unlocked(
                        node.id,
                        node.labels,
                        node.properties,
                    )?;
                }
                ("edge", None) => {
                    let edge = decode_edge_value(value)?;
                    if edge.id != id {
                        return Err(Error::invalid_argument(
                            "edge id in key does not match encoded value",
                        ));
                    }
                    self.engine.create_edge_unlocked(
                        edge.id,
                        edge.from,
                        edge.to,
                        edge.label,
                        edge.properties,
                    )?;
                }
                ("node", Some(property_key)) => {
                    self.engine
                        .set_node_property_unlocked(id, property_key, value)?;
                }
                ("edge", Some(property_key)) => {
                    let _ = property_key;
                    let _ = value;
                }
                _ => {}
            }
        }

        self.active = false;
        Ok(())
    }

    fn rollback(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.active = false;
        Ok(())
    }

    fn set_isolation(&mut self, level: IsolationLevel) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.isolation = level;
        Ok(())
    }
}

impl GraphTransaction {
    /// Typed insert of a node within this transaction.
    pub fn create_node(
        &mut self,
        id: impl Into<Vec<u8>>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let id = id.into();
        let labels: std::collections::BTreeSet<String> = labels.into_iter().map(Into::into).collect();
        let node = Node { id: id.clone(), labels, properties };
        let key = encode_node_key(&id);
        let value = encode_node_value(&node)?;
        self.local_puts.insert(key, value);
        self.local_deletes.remove(&encode_node_key(&id));
        Ok(())
    }

    /// Typed insert of an edge within this transaction.
    pub fn create_edge(
        &mut self,
        id: impl Into<Vec<u8>>,
        from: impl Into<Vec<u8>>,
        to: impl Into<Vec<u8>>,
        label: impl Into<String>,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let id = id.into();
        let edge = Edge::new(id.clone(), from, to, label, properties);
        let key = encode_edge_key(&id);
        let value = encode_edge_value(&edge)?;
        self.local_puts.insert(key, value);
        self.local_deletes.remove(&encode_edge_key(&id));
        Ok(())
    }

    /// Typed delete of a node within this transaction.
    pub fn delete_node(&mut self, id: &[u8]) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(encode_node_key(id));
        Ok(())
    }
}

fn encode_node_value(node: &Node) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(node).map_err(|e| Error::property_encoding(e.to_string()))
}

fn encode_edge_value(edge: &Edge) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(edge).map_err(|e| Error::property_encoding(e.to_string()))
}

fn decode_node_value(value: Vec<u8>) -> crate::Result<Node> {
    serde_json::from_slice(&value).map_err(|e| Error::property_encoding(e.to_string()))
}

fn decode_edge_value(value: Vec<u8>) -> crate::Result<Edge> {
    serde_json::from_slice(&value).map_err(|e| Error::property_encoding(e.to_string()))
}
