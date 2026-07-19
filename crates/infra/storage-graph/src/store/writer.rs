//! Append-only store writers for node and edge records.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::format::{RecordAddress, encode_edge_record, encode_node_record};
use crate::model::{Edge, Node};

/// Append-only writer for node records.
pub struct NodeWriter {
    file: File,
    file_id: u64,
    offset: u64,
    path: PathBuf,
}

impl NodeWriter {
    /// Open or create a node store file at `path` with `file_id`.
    pub fn open(path: impl AsRef<Path>, file_id: u64) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let offset = file.metadata()?.len();
        Ok(Self {
            file,
            file_id,
            offset,
            path,
        })
    }

    /// Append a node record and return its stable address.
    pub fn append(&mut self, node: &Node) -> crate::Result<RecordAddress> {
        let record = crate::format::NodeRecord::from(node);
        let bytes = encode_node_record(&record)?;
        let offset = self.offset;
        let len = bytes.len() as u32;
        self.file.write_all(&bytes)?;
        self.file.sync_data()?;
        self.offset += len as u64;
        Ok(RecordAddress::new(self.file_id, offset, len))
    }

    /// Return the current byte offset.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Return the file id.
    pub fn file_id(&self) -> u64 {
        self.file_id
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flush all buffered writes to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}

/// Append-only writer for edge records.
pub struct EdgeWriter {
    file: File,
    file_id: u64,
    offset: u64,
    path: PathBuf,
}

impl EdgeWriter {
    /// Open or create an edge store file at `path` with `file_id`.
    pub fn open(path: impl AsRef<Path>, file_id: u64) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        let offset = file.metadata()?.len();
        Ok(Self {
            file,
            file_id,
            offset,
            path,
        })
    }

    /// Append an edge record and return its stable address.
    pub fn append(&mut self, edge: &Edge) -> crate::Result<RecordAddress> {
        let record = crate::format::EdgeRecord::from(edge);
        let bytes = encode_edge_record(&record)?;
        let offset = self.offset;
        let len = bytes.len() as u32;
        self.file.write_all(&bytes)?;
        self.file.sync_data()?;
        self.offset += len as u64;
        Ok(RecordAddress::new(self.file_id, offset, len))
    }

    /// Return the current byte offset.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Return the file id.
    pub fn file_id(&self) -> u64 {
        self.file_id
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flush all buffered writes to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, PropertyMap};

    #[test]
    fn append_node_and_update_offset() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = NodeWriter::open(dir.path().join("nodes"), 0).unwrap();
        let node = Node::new(b"n1", ["User"], PropertyMap::new());
        let addr = writer.append(&node).unwrap();
        assert_eq!(addr.file_id, 0);
        assert_eq!(addr.offset, 0);
        assert!(addr.len > 0);
        assert!(writer.offset() > 0);
    }
}
