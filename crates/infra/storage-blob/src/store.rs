//! `BlobStoreImpl` implementation.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use storage_traits::BlobStore as BlobStoreTrait;
use storage_wal::{Durability, Wal, WalOptions};

use crate::gc::{GarbageCollector, GcWorker};
use crate::index::{BlobLocation, Index};
use crate::index_wal::IndexRecord;
use crate::volume::BlobPayloadReader;
use crate::volume_manager::VolumeManager;
use crate::{BlobStoreOptions, Error, Result};

/// Content-addressed object store.
#[derive(Debug)]
pub struct BlobStoreImpl {
    path: PathBuf,
    options: BlobStoreOptions,
    wal: Arc<Wal>,
    index: Index,
    volumes: Arc<VolumeManager>,
    gc: Arc<GarbageCollector>,
    gc_worker: Option<GcWorker>,
}

impl BlobStoreImpl {
    /// Open or create a blob store at `path`.
    pub fn open(path: impl AsRef<Path>, options: BlobStoreOptions) -> Result<Self> {
        options.validate()?;
        let path = path.as_ref().to_path_buf();
        fs::create_dir_all(&path)?;
        fs::create_dir_all(path.join("index-wal"))?;

        let wal = Arc::new(Wal::open(
            path.join("index-wal"),
            WalOptions {
                segment_size: 64 * 1024 * 1024,
                ..Default::default()
            },
        )?);
        let volumes = Arc::new(VolumeManager::open(&path, &options)?);
        let index = Index::new();

        crate::recovery::recover(&path, &wal, &index, &volumes)?;

        let gc = Arc::new(GarbageCollector::new(
            Arc::clone(&wal),
            index.clone(),
            Arc::clone(&volumes),
        ));
        let gc_worker = if options.background_gc {
            Some(GcWorker::start(Arc::clone(&gc), options.clone()))
        } else {
            None
        };

        Ok(Self {
            path,
            options,
            wal,
            index,
            volumes,
            gc,
            gc_worker,
        })
    }

    /// Return the path to the store directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Flush all durable state to stable storage.
    pub fn sync(&self) -> Result<()> {
        self.volumes.sync()?;
        // The index WAL uses Durability::Immediate for every append, so no
        // additional WAL sync is required.
        Ok(())
    }

    /// Force a GC run.  This is synchronous and may take a while.
    pub fn force_gc(&self) -> Result<()> {
        self.gc.run_once(&self.options)?;
        Ok(())
    }
}

impl Drop for BlobStoreImpl {
    fn drop(&mut self) {
        if let Some(worker) = self.gc_worker.take() {
            worker.shutdown();
        }
    }
}

impl BlobStoreTrait for BlobStoreImpl {
    type Error = Error;
    type Reader = BlobPayloadReader;
    type Writer = BlobWriter;

    fn put(&self, id: &[u8], reader: &mut dyn Read) -> Result<u64> {
        let (loc, header) = self.volumes.append_record(id, reader)?;
        let location = BlobLocation::from_record(loc.volume_number, loc.offset, &header);
        let record = IndexRecord::Put {
            id: id.to_vec(),
            volume_number: loc.volume_number,
            offset: loc.offset,
            payload_len: header.payload_len,
            payload_crc: header.payload_crc,
        };
        self.wal
            .append(record.encode(), Durability::Immediate)
            .map_err(|e| Error::IndexWal(e.to_string()))?;
        self.index.put(id.to_vec(), location);
        Ok(header.payload_len)
    }

    fn get(&self, id: &[u8]) -> Result<Self::Reader> {
        let location = self
            .index
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_vec()))?;
        let volume_reader = self.volumes.reader(location.volume_number)?;
        let (header, _record_size) = volume_reader.read_header(location.offset)?;
        let path = self.volumes.volume_path(location.volume_number);
        BlobPayloadReader::open(&path, location.offset, &header, id)
    }

    fn delete(&self, id: &[u8]) -> Result<()> {
        if self.index.get(id).is_none() {
            return Ok(());
        }
        let record = IndexRecord::Delete { id: id.to_vec() };
        self.wal
            .append(record.encode(), Durability::Immediate)
            .map_err(|e| Error::IndexWal(e.to_string()))?;
        self.index.delete(id);
        Ok(())
    }

    fn size(&self, id: &[u8]) -> Result<Option<u64>> {
        Ok(self.index.get(id).map(|l| l.payload_len))
    }
}

/// Writer type required by the `BlobStore` trait.
///
/// Currently the trait does not expose a writer-based `put`, so this type is a
/// placeholder that buffers bytes in memory.  A future `put_writer` API can use
/// a real streaming writer.
#[derive(Debug)]
pub struct BlobWriter {
    buf: Vec<u8>,
}

impl BlobWriter {
    /// Create a new in-memory writer.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }
}

impl Default for BlobWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for BlobWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
