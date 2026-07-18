//! Background dirty-page cleaner.
//!
//! The cleaner periodically scans the buffer pool and flushes dirty, unpinned
//! frames. This keeps cold dirty pages off the foreground path so that eviction
//! rarely has to write a page synchronously.
//!
//! The cleaner only flushes frames; it does not evict them. A frame that is
//! currently pinned stays resident and accessible.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::buffer::BufferPool;
use crate::error::{Error, Result};
use crate::sync::Mutex as SyncMutex;

/// Handle to the background page-cleaner thread.
pub struct PageCleaner {
    stop: Arc<AtomicBool>,
    handle: SyncMutex<Option<JoinHandle<Result<()>>>>,
}

impl PageCleaner {
    /// Spawn a background cleaner that wakes every `interval` and flushes cold
    /// dirty frames.
    ///
    /// # Panics
    ///
    /// Panics if `interval` is zero.
    pub fn spawn(pool: Arc<BufferPool>, interval: Duration) -> Self {
        assert!(
            !interval.is_zero(),
            "page cleaner interval must be non-zero"
        );
        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let handle = std::thread::spawn(move || cleaner_loop(pool, stop_clone, interval));
        Self {
            stop,
            handle: SyncMutex::new(Some(handle)),
        }
    }

    /// Signal the cleaner to stop and wait for it to finish.
    ///
    /// Errors from the background loop are surfaced here. The stop is idempotent.
    pub fn stop(&self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.with_mut(|h| {
            if let Some(handle) = h.take() {
                handle
                    .join()
                    .map_err(|_| Error::Corruption("page cleaner thread panicked".into()))??;
            }
            Ok(())
        })
    }
}

fn cleaner_loop(pool: Arc<BufferPool>, stop: Arc<AtomicBool>, interval: Duration) -> Result<()> {
    loop {
        // Sleep in small increments so shutdown is responsive even with a long
        // interval.
        let mut slept = Duration::ZERO;
        while slept < interval {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
            slept += Duration::from_millis(10);
        }

        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }

        pool.flush_dirty_unpinned()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::PagedFile;
    use crate::space::PageAllocator;
    use crate::sync::Mutex as SyncMutex;

    fn make_pool(capacity: usize) -> (Arc<BufferPool>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool = Arc::new(BufferPool::new(capacity, 512, disk, alloc).unwrap());
        (pool, dir)
    }

    #[test]
    fn cleaner_flushes_dirty_unpinned_frames() {
        let (pool, dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        let id = guard.page().id;
        guard
            .page()
            .insert(b"k", &crate::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();
        drop(guard);

        // Verify the frame is dirty before starting the cleaner.
        assert!(pool.is_frame_dirty(id).unwrap());

        let cleaner = PageCleaner::spawn(Arc::clone(&pool), Duration::from_millis(50));
        // Wait for at least one cleaner pass.
        std::thread::sleep(Duration::from_millis(150));
        cleaner.stop().unwrap();

        assert!(!pool.is_frame_dirty(id).unwrap());

        // Reopen a fresh pool over the same file and verify the page is durable.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(4, 512, disk, alloc).unwrap());
        let guard = pool2.fetch_or_read(id).unwrap();
        let cell = guard.page().get(b"k").unwrap().unwrap();
        assert_eq!(
            cell.value.as_value_kind(),
            crate::slot::ValueKind::Inline(b"v")
        );
    }

    #[test]
    fn cleaner_does_not_flush_pinned_frames() {
        let (pool, _dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        let id = guard.page().id;
        guard
            .page()
            .insert(b"k", &crate::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();

        let cleaner = PageCleaner::spawn(Arc::clone(&pool), Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(150));
        cleaner.stop().unwrap();

        // The pinned frame should still be dirty.
        assert!(pool.is_frame_dirty(id).unwrap());
        drop(guard);
    }

    #[test]
    fn cleaner_stops_immediately() {
        let (pool, _dir) = make_pool(2);
        let cleaner = PageCleaner::spawn(Arc::clone(&pool), Duration::from_secs(60));
        cleaner.stop().unwrap();
    }
}
