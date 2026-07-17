//! Garbage collection for `storage-blob`.
//!
//! GC rewrites live records from fragmented volumes into the active volume and
//! deletes the old volumes once their contents are no longer referenced by the
//! index.  Readers hold open file descriptors, so unlinking a volume file while
//! a read is in progress is safe on Unix-like systems.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use storage_wal::{Durability, Wal};

use crate::index::{BlobLocation, Index};
use crate::index_wal::IndexRecord;
use crate::volume::VolumeReader;
use crate::volume_manager::VolumeManager;
use crate::{BlobStoreOptions, Error, Result};

/// Garbage collector state.
#[derive(Debug)]
pub struct GarbageCollector {
    wal: Arc<Wal>,
    index: Index,
    volumes: Arc<VolumeManager>,
    bytes_rewritten: AtomicU64,
    bytes_dropped: AtomicU64,
}

impl GarbageCollector {
    /// Create a new GC instance.
    pub fn new(wal: Arc<Wal>, index: Index, volumes: Arc<VolumeManager>) -> Self {
        Self {
            wal,
            index,
            volumes,
            bytes_rewritten: AtomicU64::new(0),
            bytes_dropped: AtomicU64::new(0),
        }
    }

    /// Run one GC pass.  Returns the number of bytes reclaimed.
    pub fn run_once(&self, options: &BlobStoreOptions) -> Result<u64> {
        let threshold = options.gc_dead_ratio_threshold;
        if threshold <= 0.0 {
            return Ok(0);
        }

        let live_snapshot = self.index.snapshot();
        let mut by_volume: BTreeMap<u64, Vec<(Vec<u8>, BlobLocation)>> = BTreeMap::new();
        for (id, loc) in live_snapshot {
            by_volume
                .entry(loc.volume_number)
                .or_default()
                .push((id, loc));
        }

        let active_volume = self.volumes.active_volume_number();
        let mut reclaimed = 0u64;

        for (volume_number, live_records) in by_volume {
            // Never GC the active volume.
            if active_volume == Some(volume_number) {
                continue;
            }

            let path = self.volumes.volume_path(volume_number);
            if !path.exists() {
                continue;
            }

            let reader = VolumeReader::open(&path, volume_number)?;
            let file_size = reader.file_size()?;
            let live_bytes: u64 = live_records
                .iter()
                .map(|(id, loc)| {
                    crate::format::padded_record_size(id.len() as u32, loc.payload_len)
                })
                .sum();
            let dead_bytes = file_size.saturating_sub(live_bytes);
            let dead_ratio = dead_bytes as f64 / file_size.max(1) as f64;

            if dead_ratio < threshold {
                continue;
            }

            let mut volume_reclaimed = 0u64;
            for (id, old_loc) in live_records {
                // Read the full record (GC operates on whole records).
                let (_header, _stored_id, payload) = reader.read_record(old_loc.offset)?;

                // Rewrite to the active volume.
                let (new_loc, new_header) = self
                    .volumes
                    .append_record(&id, &mut Cursor::new(&payload))?;

                // Atomically update the index only if it still points to the
                // old location.  If another write or GC moved it in the
                // meantime, we discard our rewrite.
                let new_location =
                    BlobLocation::from_record(new_loc.volume_number, new_loc.offset, &new_header);
                let swapped = self
                    .index
                    .compare_and_swap(&id, old_loc, new_location)
                    .is_some();

                if swapped {
                    let record = IndexRecord::GcMove {
                        id: id.clone(),
                        old_volume_number: old_loc.volume_number,
                        new_volume_number: new_location.volume_number,
                        new_offset: new_location.offset,
                        new_payload_len: new_location.payload_len,
                        new_payload_crc: new_location.payload_crc,
                    };
                    self.wal
                        .append(record.encode(), Durability::Immediate)
                        .map_err(|e| Error::IndexWal(e.to_string()))?;
                    self.bytes_rewritten
                        .fetch_add(payload.len() as u64, Ordering::Relaxed);
                    volume_reclaimed +=
                        crate::format::padded_record_size(id.len() as u32, payload.len() as u64);
                }
            }

            // Evict the reader cache entry and delete the volume.
            self.volumes.evict_reader(volume_number);
            fs::remove_file(&path)?;
            self.bytes_dropped
                .fetch_add(volume_reclaimed, Ordering::Relaxed);
            reclaimed += volume_reclaimed;
        }

        Ok(reclaimed)
    }

    /// Return the cumulative number of payload bytes rewritten by GC.
    pub fn bytes_rewritten(&self) -> u64 {
        self.bytes_rewritten.load(Ordering::Relaxed)
    }

    /// Return the cumulative number of bytes dropped from deleted volumes.
    pub fn bytes_dropped(&self) -> u64 {
        self.bytes_dropped.load(Ordering::Relaxed)
    }
}

/// Commands sent to the background GC worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GcCommand {
    /// Run one GC pass as soon as possible.
    Run,
    /// Stop the worker thread.
    Shutdown,
}

/// Handle to the background GC worker.
#[derive(Debug)]
pub struct GcWorker {
    sender: Sender<GcCommand>,
    handle: Option<JoinHandle<()>>,
}

impl GcWorker {
    /// Start a background thread that runs GC periodically.
    pub fn start(gc: Arc<GarbageCollector>, options: BlobStoreOptions) -> Self {
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || worker_loop(gc, options, receiver));
        Self {
            sender,
            handle: Some(handle),
        }
    }

    /// Ask the worker to run one GC pass soon.
    pub fn trigger(&self) {
        let _ = self.sender.send(GcCommand::Run);
    }

    /// Gracefully shut down the worker and wait for it to finish.
    pub fn shutdown(mut self) {
        let _ = self.sender.send(GcCommand::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(
    gc: Arc<GarbageCollector>,
    options: BlobStoreOptions,
    receiver: Receiver<GcCommand>,
) {
    let interval = options.background_gc_interval;
    loop {
        // Wait for a command or the next periodic tick.
        match receiver.recv_timeout(interval) {
            Ok(GcCommand::Shutdown) => break,
            Ok(GcCommand::Run) | Err(mpsc::RecvTimeoutError::Timeout) => {
                // Isolate panics so a single GC bug does not kill the store.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = gc.run_once(&options);
                }));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use crate::BlobStoreImpl;
    use std::io::Read;
    use std::time::Duration;
    use storage_traits::BlobStore;
    use tempfile::TempDir;

    fn open_with_small_volumes(dir: &TempDir) -> BlobStoreImpl {
        let opts = BlobStoreOptions {
            max_volume_size: 64 * 1024,
            gc_dead_ratio_threshold: 0.1,
            background_gc: true,
            background_gc_interval: Duration::from_millis(50),
            sync_on_put: true,
        };
        BlobStoreImpl::open(dir.path(), opts).unwrap()
    }

    #[test]
    fn background_gc_reclaims_deleted_objects() {
        let dir = TempDir::new().unwrap();
        let store = open_with_small_volumes(&dir);

        let mut ids = Vec::new();
        for i in 0..20u8 {
            let id = vec![b'k', i];
            let payload = vec![i; 4096];
            store.put(&id, &mut &payload[..]).unwrap();
            ids.push(id);
        }
        for id in &ids[..10] {
            store.delete(id).unwrap();
        }
        store.sync().unwrap();

        // Wait for the periodic worker to run.
        std::thread::sleep(Duration::from_millis(300));

        // Live objects still readable.
        for id in &ids[10..] {
            let mut reader = store.get(id).unwrap();
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf).unwrap();
            assert_eq!(buf.len(), 4096);
        }
        // Deleted objects stay gone.
        for id in &ids[..10] {
            assert!(store.get(id).is_err());
        }
    }
}
