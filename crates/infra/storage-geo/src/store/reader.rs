//! Positional reader for the feature store.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::feature::Feature;
use crate::format::FeatureRecord;

/// Reader for a single feature store file.
pub struct FeatureReader {
    file: File,
}

impl FeatureReader {
    /// Open the store file at `path`.
    pub fn open(path: &Path) -> crate::Result<Self> {
        let file = File::open(path)?;
        Ok(Self { file })
    }

    /// Read exactly `len` bytes at `offset` from the file.
    pub fn read_at(path: &Path, offset: u64, len: u32) -> crate::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len as usize];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Read the next feature record from the current file position.
    fn read_next(&mut self) -> crate::Result<Option<Feature>> {
        let mut len_buf = [0u8; 4];
        match self.file.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let record_len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; record_len];
        self.file.read_exact(&mut body)?;
        let mut data = Vec::with_capacity(4 + record_len);
        data.extend_from_slice(&len_buf);
        data.extend_from_slice(&body);
        let feature = FeatureRecord::decode(&data)?;
        Ok(Some(feature))
    }
}

impl Iterator for FeatureReader {
    type Item = crate::Result<Feature>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.read_next() {
            Ok(Some(f)) => Some(Ok(f)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

