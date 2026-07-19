//! Persistent vector page storage.
//!
//! Vectors are kept in memory for fast index access and periodically flushed to
//! page files. Because the WAL durably logs every write, the page files are
//! only a cache: on recovery the engine replays the WAL into memory and can
//! rewrite the pages.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;

use crate::error::Error;
use crate::format::{PageHeader, VectorRecord, VECTOR_DIR};
use crate::options::VectorOptions;
use crate::options::Quantization;
use crate::quantization::ScalarQuantizer;

/// In-memory vector storage with optional scalar quantization.
pub struct VectorStorage {
    options: VectorOptions,
    dir: PathBuf,
    vectors: RwLock<HashMap<u64, VectorRecord>>,
    quantizer: RwLock<Option<ScalarQuantizer>>,
    page_counter: RwLock<u64>,
}

impl VectorStorage {
    /// Create a new in-memory storage instance.
    pub fn new(options: VectorOptions, dir: impl AsRef<Path>) -> Self {
        Self {
            options,
            dir: dir.as_ref().to_path_buf(),
            vectors: RwLock::new(HashMap::new()),
            quantizer: RwLock::new(None),
            page_counter: RwLock::new(0),
        }
    }

    /// Insert or replace a vector record.
    pub fn put(&self, record: VectorRecord) -> crate::Result<()> {
        if record.vector.len() != self.options.dimension {
            return Err(Error::dimension_mismatch(
                self.options.dimension,
                record.vector.len(),
            ));
        }
        self.vectors.write().insert(record.id, record);
        Ok(())
    }

    /// Remove a vector by internal id.
    pub fn delete(&self, id: u64) -> Option<VectorRecord> {
        self.vectors.write().remove(&id)
    }

    /// Get a vector by internal id.
    pub fn get(&self, id: u64) -> Option<VectorRecord> {
        self.vectors.read().get(&id).cloned()
    }

    /// Return the number of stored vectors.
    pub fn len(&self) -> usize {
        self.vectors.read().len()
    }

    /// Return whether the storage is empty.
    pub fn is_empty(&self) -> bool {
        self.vectors.read().is_empty()
    }

    /// Return all stored records.
    pub fn records(&self) -> Vec<VectorRecord> {
        self.vectors.read().values().cloned().collect()
    }

    /// Return the raw vector for an id.
    pub fn get_vector(&self, id: u64) -> Option<Vec<f32>> {
        self.vectors.read().get(&id).map(|r| r.vector.clone())
    }

    /// Build the scalar quantizer from the current dataset.
    pub fn rebuild_quantizer(&self) -> crate::Result<()> {
        if self.options.quantization != Quantization::Scalar {
            *self.quantizer.write() = None;
            return Ok(());
        }
        let records = self.records();
        if records.is_empty() {
            *self.quantizer.write() = None;
            return Ok(());
        }
        let vectors: Vec<Vec<f32>> = records.into_iter().map(|r| r.vector).collect();
        let quantizer = ScalarQuantizer::fit(&vectors).ok_or_else(|| {
            Error::InvalidArgument("not enough vectors to fit scalar quantizer".into())
        })?;
        *self.quantizer.write() = Some(quantizer);
        Ok(())
    }

    /// Flush all in-memory vectors to page files.
    pub fn flush(&self) -> crate::Result<()> {
        let records = self.records();
        if records.is_empty() {
            return Ok(());
        }

        std::fs::create_dir_all(self.dir.join(VECTOR_DIR))?;

        // Simple strategy: write one page file per flush, capping by page size.
        let mut pages: Vec<Vec<&VectorRecord>> = Vec::new();
        let mut current: Vec<&VectorRecord> = Vec::new();
        let mut current_size = 0usize;

        for rec in &records {
            let encoded_size = rec.encode().len();
            if current_size + encoded_size > self.options.vector_page_size && !current.is_empty() {
                pages.push(current);
                current = Vec::new();
                current_size = 0;
            }
            current_size += encoded_size;
            current.push(rec);
        }
        if !current.is_empty() {
            pages.push(current);
        }

        let mut counter = *self.page_counter.read();
        for page in &pages {
            let path = self.page_path(counter);
            self.write_page(&path, page)?;
            counter += 1;
        }
        *self.page_counter.write() = counter;

        storage_file::sync_dir(&self.dir)?;
        Ok(())
    }

    /// Load all vectors from page files in the directory.
    pub fn load(&self) -> crate::Result<()> {
        let vector_dir = self.dir.join(VECTOR_DIR);
        if !vector_dir.exists() {
            return Ok(());
        }

        let mut entries: Vec<PathBuf> = std::fs::read_dir(&vector_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.starts_with("page-"))
                    .unwrap_or(false)
            })
            .collect();
        entries.sort();

        let mut max_counter: u64 = 0;
        for path in entries {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str())
                && let Some(num_str) = name.strip_prefix("page-")
                && let Ok(n) = num_str.parse::<u64>()
                && n > max_counter
            {
                max_counter = n;
            }
            let records = self.read_page(&path)?;
            let mut vectors = self.vectors.write();
            for rec in records {
                vectors.insert(rec.id, rec);
            }
        }
        *self.page_counter.write() = max_counter + 1;
        Ok(())
    }

    fn page_path(&self, counter: u64) -> PathBuf {
        self.dir.join(VECTOR_DIR).join(format!("page-{counter:06}.vec"))
    }

    fn write_page(&self, path: &Path, records: &[&VectorRecord]) -> crate::Result<()> {
        let mut buf = Vec::with_capacity(self.options.vector_page_size);
        let header = PageHeader {
            magic: crate::format::MAGIC,
            version: crate::format::VERSION,
            count: records.len() as u32,
            dimension: self.options.dimension as u32,
            data_offset: 20,
        };
        buf.extend_from_slice(&header.encode());
        for rec in records {
            buf.extend_from_slice(&rec.encode());
        }
        storage_file::atomic_write(path, &buf)?;
        Ok(())
    }

    fn read_page(&self, path: &Path) -> crate::Result<Vec<VectorRecord>> {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if buf.len() < 20 {
            return Err(Error::corruption("page file too short"));
        }
        let header = PageHeader::decode(&buf[..20])?;
        if header.dimension as usize != self.options.dimension {
            return Err(Error::dimension_mismatch(
                self.options.dimension,
                header.dimension as usize,
            ));
        }
        let mut records = Vec::with_capacity(header.count as usize);
        let mut offset = header.data_offset as usize;
        for _ in 0..header.count {
            let rec = VectorRecord::decode(&buf[offset..])?;
            offset += rec.encode().len();
            records.push(rec);
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> VectorOptions {
        VectorOptions::brute_force(3, crate::distance::DistanceMetric::Euclidean)
    }

    #[test]
    fn put_get_delete() {
        let dir = tempfile::tempdir().unwrap();
        let storage = VectorStorage::new(opts(), dir.path());
        let rec = VectorRecord {
            id: 1,
            key: b"a".to_vec(),
            vector: vec![1.0f32, 2.0, 3.0],
        };
        storage.put(rec.clone()).unwrap();
        assert_eq!(storage.get(1).unwrap().vector, rec.vector);
        assert!(storage.delete(1).is_some());
        assert!(storage.get(1).is_none());
    }

    #[test]
    fn flush_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let storage = VectorStorage::new(opts(), dir.path());
        for i in 0..100u64 {
            storage
                .put(VectorRecord {
                    id: i + 1,
                    key: format!("k{i}").into_bytes(),
                    vector: vec![i as f32, (i * 2) as f32, (i * 3) as f32],
                })
                .unwrap();
        }
        storage.flush().unwrap();

        let loaded = VectorStorage::new(opts(), dir.path());
        loaded.load().unwrap();
        assert_eq!(loaded.len(), 100);
        for i in 0..100u64 {
            let rec = loaded.get(i + 1).unwrap();
            assert_eq!(rec.key, format!("k{i}").into_bytes());
        }
    }
}
