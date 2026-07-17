//! Scan-pin reference counting for safe file deletion during compaction.
//!
//! Problem: compaction writes new Parquet files and removes old paths from the
//! manifest, but a scan that started before the compaction may still hold an
//! open file descriptor to an obsolete file. Deleting the file immediately
//! would cause those scans to fail.
//!
//! Solution: each scan pins the manifest it is reading. A file is only unlinked
//! after it has been removed from the manifest *and* no pinned manifest
//! references it. This is a lightweight, epoch-based scheme.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::Result;
use crate::manifest::Manifest;

/// Set of active scan pins and a queue of files awaiting deletion.
#[derive(Debug, Default)]
pub struct PinSet {
    inner: Mutex<PinSetInner>,
}

#[derive(Debug, Default)]
struct PinSetInner {
    /// Number of active scans pinned to each manifest epoch. The epoch is the
    /// manifest's identity (its Arc pointer value), which is stable because the
    /// manifest is immutable once published.
    pins: HashMap<ArcPointer, usize>,
    /// Files that have been removed from the manifest and are waiting for all
    /// pinned epochs that referenced them to be released.
    pending: Vec<PendingDeletion>,
}

/// Type-erased Arc pointer identity. We only use the raw pointer value as an
/// epoch identifier; the manifest is never accessed through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ArcPointer(usize);

#[derive(Debug)]
struct PendingDeletion {
    paths: Vec<PathBuf>,
    /// Epochs that were active when the file was retired. The file can be
    /// deleted only after all of these epochs have zero pins.
    epochs: HashSet<ArcPointer>,
}

impl PinSet {
    /// Create an empty pin set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin a manifest and return a guard that releases the pin on drop.
    pub fn pin(&self, manifest: &Arc<Manifest>) -> PinGuard {
        let epoch = ArcPointer(Arc::as_ptr(manifest) as usize);
        let mut inner = self.inner.lock();
        *inner.pins.entry(epoch).or_insert(0) += 1;
        PinGuard {
            set: self,
            epoch,
            released: false,
        }
    }

    /// Record that `paths` are no longer in the manifest and may be deleted
    /// once no scan references them.
    pub fn retire_files(&self, paths: Vec<PathBuf>) {
        let mut inner = self.inner.lock();
        let epochs: HashSet<ArcPointer> = inner.pins.keys().copied().collect();
        inner.pending.push(PendingDeletion { paths, epochs });
    }

    /// Attempt to delete any pending files whose epochs have all been released.
    pub fn reap(&self, _table_path: &Path) -> Result<()> {
        let mut inner = self.inner.lock();
        // Copy the current pin counts so we can mutate `pending` while reading
        // the immutable pin set.
        let pins_snapshot: HashMap<ArcPointer, usize> = inner.pins.clone();
        let mut still_pending = Vec::new();

        for pending in inner.pending.drain(..) {
            let safe = pending
                .epochs
                .iter()
                .all(|epoch| pins_snapshot.get(epoch).copied().unwrap_or(0) == 0);
            if safe {
                for path in pending.paths {
                    let _ = std::fs::remove_file(&path);
                }
            } else {
                still_pending.push(pending);
            }
        }

        inner.pending = still_pending;
        Ok(())
    }

    fn unpin(&self, epoch: ArcPointer) {
        let mut inner = self.inner.lock();
        if let Some(count) = inner.pins.get_mut(&epoch) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                inner.pins.remove(&epoch);
            }
        }
    }

    /// Return the number of files waiting for their epochs to expire.
    /// Used only by tests.
    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.inner.lock().pending.len()
    }
}

/// Guard that releases a manifest pin when dropped.
pub struct PinGuard {
    set: *const PinSet,
    epoch: ArcPointer,
    released: bool,
}

impl Drop for PinGuard {
    fn drop(&mut self) {
        if !self.released {
            // SAFETY: `PinSet` outlives any scan because it is owned by the
            // engine, and `PinGuard` is only created from a `&PinSet` owned by
            // the engine. The pointer is never used after the engine is dropped
            // because guards are not leaked across the engine's lifetime.
            unsafe { (*self.set).unpin(self.epoch) };
        }
    }
}

// PinGuard is not Send/Sync by design: it should stay on the thread that
// created the scan.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use crate::schema::TableSchema;
    use std::sync::Arc;

    #[test]
    fn pending_files_wait_for_pin_release() {
        let set = PinSet::new();
        let manifest = Arc::new(Manifest {
            schema: TableSchema::empty(),
            files: Vec::new(),
        });

        let pin = set.pin(&manifest);
        set.retire_files(vec![PathBuf::from("/tmp/old.parquet")]);
        assert_eq!(set.pending_count(), 1);

        // Reaping while pinned should not delete the file (we can't easily assert
        // deletion without a real file, but pending_count should stay non-zero).
        set.reap(Path::new("/tmp")).unwrap();
        assert_eq!(set.pending_count(), 1);

        drop(pin);
        set.reap(Path::new("/tmp")).unwrap();
        assert_eq!(set.pending_count(), 0);
    }
}
