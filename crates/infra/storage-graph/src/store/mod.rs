//! Append-only node and edge stores.

use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::format::RecordAddress;
use crate::model::{Edge, Node};
use crate::store::reader::{EdgeReader, NodeReader};
use crate::store::writer::{EdgeWriter, NodeWriter};

pub mod reader;
pub mod writer;

/// Append-only store for node records.
pub struct NodeStore {
    path: PathBuf,
    file_id: u64,
    writer: Mutex<NodeWriter>,
}

impl NodeStore {
    /// Open or create a node store at `path` with `file_id`.
    pub fn open(path: impl AsRef<Path>, file_id: u64) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let writer = NodeWriter::open(&path, file_id)?;
        Ok(Self {
            path,
            file_id,
            writer: Mutex::new(writer),
        })
    }

    /// Append a node record and return its stable address.
    pub fn append(&self, node: &Node) -> crate::Result<RecordAddress> {
        self.writer.lock().append(node)
    }

    /// Read a node by its record address.
    pub fn get(&self, address: RecordAddress) -> crate::Result<Option<Node>> {
        let mut reader = NodeReader::open(&self.path)?;
        match reader.get(address)? {
            Some(record) => Ok(Some(record.into())),
            None => Ok(None),
        }
    }

    /// Iterate over all node records in the store.
    pub fn iter(&self) -> crate::Result<impl Iterator<Item = crate::Result<(RecordAddress, Node)>>> {
        let mut reader = NodeReader::open(&self.path)?;
        let items: Vec<_> = reader.iter().collect();
        Ok(items.into_iter().map(|res| res.map(|(addr, rec)| (addr, rec.into()))))
    }

    /// Flush all buffered writes to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.writer.lock().sync()
    }

    /// Return the current file id.
    pub fn file_id(&self) -> u64 {
        self.file_id
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replace the underlying store file with a new one at `path`.
    pub fn replace(&self, path: impl AsRef<Path>, file_id: u64) -> crate::Result<()> {
        let path = path.as_ref().to_path_buf();
        let writer = NodeWriter::open(&path, file_id)?;
        *self.writer.lock() = writer;
        Ok(())
    }
}

/// Append-only store for edge records.
pub struct EdgeStore {
    path: PathBuf,
    file_id: u64,
    writer: Mutex<EdgeWriter>,
}

impl EdgeStore {
    /// Open or create an edge store at `path` with `file_id`.
    pub fn open(path: impl AsRef<Path>, file_id: u64) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let writer = EdgeWriter::open(&path, file_id)?;
        Ok(Self {
            path,
            file_id,
            writer: Mutex::new(writer),
        })
    }

    /// Append an edge record and return its stable address.
    pub fn append(&self, edge: &Edge) -> crate::Result<RecordAddress> {
        self.writer.lock().append(edge)
    }

    /// Read an edge by its record address.
    pub fn get(&self, address: RecordAddress) -> crate::Result<Option<Edge>> {
        let mut reader = EdgeReader::open(&self.path)?;
        match reader.get(address)? {
            Some(record) => Ok(Some(record.into())),
            None => Ok(None),
        }
    }

    /// Iterate over all edge records in the store.
    pub fn iter(&self) -> crate::Result<impl Iterator<Item = crate::Result<(RecordAddress, Edge)>>> {
        let mut reader = EdgeReader::open(&self.path)?;
        let items: Vec<_> = reader.iter().collect();
        Ok(items.into_iter().map(|res| res.map(|(addr, rec)| (addr, rec.into()))))
    }

    /// Flush all buffered writes to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.writer.lock().sync()
    }

    /// Return the current file id.
    pub fn file_id(&self) -> u64 {
        self.file_id
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replace the underlying store file with a new one at `path`.
    pub fn replace(&self, path: impl AsRef<Path>, file_id: u64) -> crate::Result<()> {
        let path = path.as_ref().to_path_buf();
        let writer = EdgeWriter::open(&path, file_id)?;
        *self.writer.lock() = writer;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, PropertyMap};

    #[test]
    fn node_store_append_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let store = NodeStore::open(dir.path().join("nodes"), 0).unwrap();
        let node = Node::new(b"n1", ["User"], PropertyMap::new());
        let addr = store.append(&node).unwrap();
        let got = store.get(addr).unwrap().unwrap();
        assert_eq!(node, got);
    }

    #[test]
    fn edge_store_append_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let store = EdgeStore::open(dir.path().join("edges"), 0).unwrap();
        let edge = Edge::new(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new());
        let addr = store.append(&edge).unwrap();
        let got = store.get(addr).unwrap().unwrap();
        assert_eq!(edge, got);
    }

    #[test]
    fn node_store_iter() {
        let dir = tempfile::tempdir().unwrap();
        let store = NodeStore::open(dir.path().join("nodes"), 0).unwrap();
        let a = Node::new(b"a", ["A"], PropertyMap::new());
        let b = Node::new(b"b", ["B"], PropertyMap::new());
        store.append(&a).unwrap();
        store.append(&b).unwrap();
        let items: Vec<_> = store.iter().unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1.id, b"a");
        assert_eq!(items[1].1.id, b"b");
    }
}
