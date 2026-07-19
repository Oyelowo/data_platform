//! Append-only feature store.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::feature::Feature;
use crate::format::{store_file_name, FeatureRecord};

pub mod reader;
pub mod writer;

pub use reader::FeatureReader;
pub use writer::FeatureWriter;

/// Address of a feature record in a store file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FeatureAddress {
    /// Store file id.
    pub file_id: u32,
    /// Byte offset of the record within the file.
    pub offset: u64,
    /// Total length of the record in bytes.
    pub len: u32,
}

/// Append-only feature store.
///
/// The store owns a single file and appends encoded feature records to it.
/// Reads are performed through separate file handles so that writes and reads
/// can proceed concurrently.
#[derive(Clone)]
pub struct FeatureStore {
    dir: PathBuf,
    file_id: u32,
    writer: Arc<Mutex<FeatureWriter>>,
}

impl FeatureStore {
    /// Open or create the feature store with the given `file_id`.
    pub fn open(dir: impl AsRef<Path>, file_id: u32) -> crate::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let path = dir.join(store_file_name(file_id));
        let writer = FeatureWriter::open(&path)?;
        Ok(Self {
            dir,
            file_id,
            writer: Arc::new(Mutex::new(writer)),
        })
    }

    /// Append a feature to the store and return its stable address.
    pub fn insert(&self, feature: &Feature) -> crate::Result<FeatureAddress> {
        let bytes = FeatureRecord::encode(feature)?;
        let mut writer = self.writer.lock();
        let offset = writer.append(&bytes)?;
        Ok(FeatureAddress {
            file_id: self.file_id,
            offset,
            len: bytes.len() as u32,
        })
    }

    /// Read a feature by address.
    pub fn get(&self, address: FeatureAddress) -> crate::Result<Option<Feature>> {
        if address.file_id != self.file_id {
            return Err(crate::Error::corruption("address belongs to a different store file"));
        }
        let path = self.dir.join(store_file_name(self.file_id));
        let data = FeatureReader::read_at(&path, address.offset, address.len)?;
        let feature = FeatureRecord::decode(&data)?;
        Ok(Some(feature))
    }

    /// Return an iterator over all records in the store.
    pub fn iter(&self) -> crate::Result<FeatureReader> {
        let path = self.dir.join(store_file_name(self.file_id));
        FeatureReader::open(&path)
    }

    /// Flush all buffered writes to stable storage.
    pub fn sync(&self) -> crate::Result<()> {
        self.writer.lock().sync()
    }

    /// Return the current store file id.
    pub fn file_id(&self) -> u32 {
        self.file_id
    }

    /// Return the path of the current store file.
    pub fn path(&self) -> PathBuf {
        self.dir.join(store_file_name(self.file_id))
    }

    /// Return the approximate size of the current store file in bytes.
    pub fn file_size(&self) -> crate::Result<u64> {
        let metadata = std::fs::metadata(self.path())?;
        Ok(metadata.len())
    }
}
