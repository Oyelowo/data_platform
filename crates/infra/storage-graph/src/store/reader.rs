//! Positional readers for node and edge store files.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::format::{EdgeRecord, NodeRecord, RecordAddress, decode_edge_record, decode_node_record};

/// Reader for node store files.
pub struct NodeReader {
    file: File,
    path: PathBuf,
}

impl NodeReader {
    /// Open an existing node store file.
    pub fn open(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        Ok(Self { file, path })
    }

    /// Read a node record at `address`.
    pub fn get(&mut self, address: RecordAddress) -> crate::Result<Option<NodeRecord>> {
        let len = address.len as usize;
        let mut buf = vec![0u8; len];
        self.file
            .seek(SeekFrom::Start(address.offset))
            .map_err(crate::Error::Io)?;
        match self.file.read_exact(&mut buf) {
            Ok(()) => Ok(Some(decode_node_record(&buf)?)),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(crate::Error::Io(e)),
        }
    }

    /// Iterate over all node records in the store.
    pub fn iter(&mut self) -> impl Iterator<Item = crate::Result<(RecordAddress, NodeRecord)>> + '_ {
        NodeIter { reader: self, offset: 0 }
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

struct NodeIter<'a> {
    reader: &'a mut NodeReader,
    offset: u64,
}

impl Iterator for NodeIter<'_> {
    type Item = crate::Result<(RecordAddress, NodeRecord)>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut len_buf = [0u8; 4];
        if let Err(e) = self
            .reader
            .file
            .seek(SeekFrom::Start(self.offset))
            .map_err(crate::Error::Io)
        {
            return Some(Err(e));
        }
        match self.reader.file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Err(e) => return Some(Err(crate::Error::Io(e))),
        }
        let payload_len = u32::from_le_bytes(len_buf) as usize;
        let total_len = 4 + payload_len + 4;
        let mut buf = vec![0u8; total_len];
        buf[..4].copy_from_slice(&len_buf);
        match self.reader.file.read_exact(&mut buf[4..]) {
            Ok(()) => {}
            Err(e) => return Some(Err(crate::Error::Io(e))),
        }
        let address = RecordAddress::new(0, self.offset, total_len as u32);
        match decode_node_record(&buf) {
            Ok(record) => {
                self.offset += total_len as u64;
                Some(Ok((address, record)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Reader for edge store files.
pub struct EdgeReader {
    file: File,
    path: PathBuf,
}

impl EdgeReader {
    /// Open an existing edge store file.
    pub fn open(path: impl AsRef<Path>) -> crate::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        Ok(Self { file, path })
    }

    /// Read an edge record at `address`.
    pub fn get(&mut self, address: RecordAddress) -> crate::Result<Option<EdgeRecord>> {
        let len = address.len as usize;
        let mut buf = vec![0u8; len];
        self.file
            .seek(SeekFrom::Start(address.offset))
            .map_err(crate::Error::Io)?;
        match self.file.read_exact(&mut buf) {
            Ok(()) => Ok(Some(decode_edge_record(&buf)?)),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(crate::Error::Io(e)),
        }
    }

    /// Iterate over all edge records in the store.
    pub fn iter(&mut self) -> impl Iterator<Item = crate::Result<(RecordAddress, EdgeRecord)>> + '_ {
        EdgeIter { reader: self, offset: 0 }
    }

    /// Path to the store file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

struct EdgeIter<'a> {
    reader: &'a mut EdgeReader,
    offset: u64,
}

impl Iterator for EdgeIter<'_> {
    type Item = crate::Result<(RecordAddress, EdgeRecord)>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut len_buf = [0u8; 4];
        if let Err(e) = self
            .reader
            .file
            .seek(SeekFrom::Start(self.offset))
            .map_err(crate::Error::Io)
        {
            return Some(Err(e));
        }
        match self.reader.file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Err(e) => return Some(Err(crate::Error::Io(e))),
        }
        let payload_len = u32::from_le_bytes(len_buf) as usize;
        let total_len = 4 + payload_len + 4;
        let mut buf = vec![0u8; total_len];
        buf[..4].copy_from_slice(&len_buf);
        match self.reader.file.read_exact(&mut buf[4..]) {
            Ok(()) => {}
            Err(e) => return Some(Err(crate::Error::Io(e))),
        }
        let address = RecordAddress::new(0, self.offset, total_len as u32);
        match decode_edge_record(&buf) {
            Ok(record) => {
                self.offset += total_len as u64;
                Some(Ok((address, record)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}
