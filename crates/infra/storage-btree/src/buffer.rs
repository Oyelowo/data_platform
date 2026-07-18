//! Frame-based buffer pool.
//!
//! The buffer pool caches on-disk pages in memory, tracks dirty pages, and
//! evicts cold pages using an adaptive CLOCK-Pro policy by default.  Each
//! frame holds an `Arc<Page>`, so threads can release the frame metadata lock
//! while retaining a logical pin on the page.  The per-page Optimistic Lock
//! Coupling (OLC) latch in `Page::latch_word` serialises concurrent access to
//! the page contents.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::{Mutex as ParkingMutex, RwLock};

use crate::disk::PagedFile;
use crate::error::{Error, Result};
use crate::eviction::{ClockPro, EvictionPolicy};
use crate::metrics::Metrics;
use crate::page::{NULL_PAGE_ID, Page, PageId};
use crate::space::PageAllocator;
use crate::sync::Mutex as SyncMutex;

/// Index of a frame inside the buffer pool.
pub type FrameId = usize;

/// One slot in the buffer pool.
pub struct Frame {
    /// Page currently stored in this frame, or `NULL_PAGE_ID` if empty.
    pub page_id: PageId,
    /// The in-memory page.  Held in an `Arc` so that threads can keep a
    /// reference after releasing the frame lock.
    pub page: Arc<Page>,
    /// Number of active logical pins on this frame.  Eviction skips frames
    /// with a non-zero pin count.
    pub pin_count: AtomicU32,
    /// Whether the frame has been modified since read from disk.
    pub dirty: AtomicBool,
    /// CLOCK reference bit.
    pub ref_bit: AtomicBool,
    /// CLOCK-Pro hot-page flag.
    hot: AtomicBool,
    /// CLOCK-Pro cold-page test-period flag.
    cold_test: AtomicBool,
    /// Approximate access frequency; bumped on every hit.
    usage_count: AtomicU32,
}

impl Frame {
    fn empty(frame_id: FrameId, page_size: usize) -> Result<Self> {
        Ok(Self {
            page_id: NULL_PAGE_ID,
            page: Arc::new(Page::new(PageId::new(frame_id as u64), page_size)?),
            pin_count: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
            ref_bit: AtomicBool::new(false),
            hot: AtomicBool::new(false),
            cold_test: AtomicBool::new(false),
            usage_count: AtomicU32::new(0),
        })
    }

    /// True if this frame is classified as hot by CLOCK-Pro.
    pub fn is_hot(&self) -> bool {
        self.hot.load(Ordering::Relaxed)
    }

    /// Set the hot classification.
    pub fn set_hot(&self, hot: bool) {
        self.hot.store(hot, Ordering::Relaxed);
    }

    /// True if this frame is in its CLOCK-Pro cold-page test period.
    pub fn is_cold_test(&self) -> bool {
        self.cold_test.load(Ordering::Relaxed)
    }

    /// Set the cold-test classification.
    pub fn set_cold_test(&self, test: bool) {
        self.cold_test.store(test, Ordering::Relaxed);
    }

    /// Increment the approximate usage counter.
    pub fn bump_usage(&self) {
        self.usage_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// RAII handle pinning a frame in the buffer pool.
///
/// A `PageGuard` holds a logical pin on a frame and an `Arc<Page>` to the
/// page stored there.  The frame metadata lock is *not* held while the guard
/// is alive; only the OLC latch inside the page protects the page contents.
pub struct PageGuard {
    frame_id: FrameId,
    page: Arc<Page>,
    pool: Arc<BufferPool>,
}

impl PageGuard {
    /// Create a guard for an existing resident page (cache hit).
    fn new(frame_id: FrameId, page: Arc<Page>, pool: Arc<BufferPool>) -> Self {
        let frame = pool.frames[frame_id].lock();
        frame.pin_count.fetch_add(1, Ordering::Relaxed);
        frame.ref_bit.store(true, Ordering::Relaxed);
        pool.metrics.inc_cache_hits();
        pool.eviction_policy.record_access(frame_id, &frame);
        drop(frame);
        Self {
            frame_id,
            page,
            pool,
        }
    }

    /// Create a guard for a page that was just installed from disk or newly
    /// allocated.  Does not count as a cache hit.
    fn new_installed(frame_id: FrameId, page: Arc<Page>, pool: Arc<BufferPool>) -> Self {
        let frame = pool.frames[frame_id].lock();
        frame.pin_count.fetch_add(1, Ordering::Relaxed);
        drop(frame);
        Self {
            frame_id,
            page,
            pool,
        }
    }

    /// Immutable access to the page.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// Return a clone of the page `Arc`.  Useful for optimistic traversals
    /// that need to keep the page resident while releasing the guard.
    pub fn page_arc(&self) -> Arc<Page> {
        Arc::clone(&self.page)
    }

    /// Mark the frame dirty.
    pub fn mark_dirty(&self) {
        self.pool.frames[self.frame_id]
            .lock()
            .dirty
            .store(true, Ordering::Relaxed);
    }

    /// True if the frame is dirty.
    pub fn is_dirty(&self) -> bool {
        self.pool.frames[self.frame_id]
            .lock()
            .dirty
            .load(Ordering::Relaxed)
    }

    /// Run a read-only closure on the pinned page and release the pin.
    ///
    /// This is the preferred pattern for short reads: the frame pin is held only
    /// for the duration of `f`, so the page cannot be evicted while the closure
    /// runs, but the pin is released immediately afterwards.
    pub fn with<R>(&self, f: impl FnOnce(&Page) -> R) -> R {
        f(self.page())
    }

    /// Run a mutating closure on the pinned page while holding its OLC write
    /// latch, then mark the frame dirty and release the latch.
    ///
    /// This helper prevents the common mistake of acquiring a write guard and
    /// forgetting to mark the frame dirty.  The closure receives a `&Page`:
    /// `Page`'s mutation methods are `&self` and rely on the exclusive latch
    /// held by this guard for serialisation.
    ///
    /// The helper retries `try_write` with exponential backoff plus jitter;
    /// only if the latch remains contended after many retries does it return
    /// `Error::Contention`.  This matches the standard OLC pattern: latches are
    /// held for very short durations, so yielding and retrying is almost always
    /// sufficient.  Backoff keeps CPU usage low under high contention and the
    /// jitter reduces thundering-herd collisions between threads racing for the
    /// same page.
    pub fn with_mut<R>(&self, f: impl FnOnce(&Page) -> Result<R>) -> Result<R> {
        const MAX_RETRIES: usize = 1024;
        const MIN_BACKOFF_NS: u64 = 100;
        const MAX_BACKOFF_NS: u64 = 1_000_000; // 1 ms
        let mut guard = None;
        let mut backoff_ns = MIN_BACKOFF_NS;
        for attempt in 0..MAX_RETRIES {
            if let Some(g) = self.page.try_write() {
                guard = Some(g);
                break;
            }
            // Exponential backoff capped at MAX_BACKOFF_NS, with a small amount
            // of jitter derived from the current instant to desynchronize
            // contending threads without adding a rand dependency.
            let jitter_ns = std::time::Instant::now()
                .elapsed()
                .as_nanos()
                .wrapping_add(attempt as u128) as u64
                % backoff_ns;
            std::thread::sleep(std::time::Duration::from_nanos(backoff_ns + jitter_ns));
            backoff_ns = (backoff_ns * 2).min(MAX_BACKOFF_NS);
        }
        let guard = guard.ok_or_else(|| {
            Error::Contention("page write latch remained contended after retries".into())
        })?;
        let result = f(guard.page());
        if result.is_ok() {
            self.mark_dirty();
        }
        // `guard` is dropped here, releasing the OLC lock and bumping the version.
        result
    }
}

impl Drop for PageGuard {
    fn drop(&mut self) {
        self.pool.frames[self.frame_id]
            .lock()
            .pin_count
            .fetch_sub(1, Ordering::Release);
    }
}

/// A frame-based buffer pool backed by a `PagedFile`.
pub struct BufferPool {
    page_size: usize,
    disk: Arc<PagedFile>,
    allocator: Arc<SyncMutex<PageAllocator>>,
    frames: Vec<ParkingMutex<Frame>>,
    /// Sharded page table: `PageId -> FrameId`.
    page_table: Vec<RwLock<HashMap<PageId, FrameId>>>,
    /// Serializes eviction and new-page installation to avoid deadlocks with
    /// the sharded page table.
    eviction_lock: ParkingMutex<()>,
    /// Operational metrics.
    metrics: Arc<Metrics>,
    /// Eviction policy.  Default is CLOCK-Pro.
    eviction_policy: Box<dyn EvictionPolicy>,
}

impl BufferPool {
    const NUM_SHARDS: usize = 64;

    /// Create a buffer pool with `capacity` frames using the default
    /// CLOCK-Pro eviction policy and a private metrics collector.
    pub fn new(
        capacity: usize,
        page_size: usize,
        disk: Arc<PagedFile>,
        allocator: Arc<SyncMutex<PageAllocator>>,
    ) -> Result<Self> {
        Self::with_metrics(
            capacity,
            page_size,
            disk,
            allocator,
            Arc::new(Metrics::new()),
        )
    }

    /// Create a buffer pool with an explicit shared metrics collector and the
    /// default CLOCK-Pro eviction policy.
    pub fn with_metrics(
        capacity: usize,
        page_size: usize,
        disk: Arc<PagedFile>,
        allocator: Arc<SyncMutex<PageAllocator>>,
        metrics: Arc<Metrics>,
    ) -> Result<Self> {
        Self::with_policy(
            capacity,
            page_size,
            disk,
            allocator,
            metrics,
            Box::new(ClockPro::new(capacity)),
        )
    }

    /// Create a buffer pool with an explicit eviction policy and metrics
    /// collection.  Primarily useful for tests.
    pub fn with_policy(
        capacity: usize,
        page_size: usize,
        disk: Arc<PagedFile>,
        allocator: Arc<SyncMutex<PageAllocator>>,
        metrics: Arc<Metrics>,
        policy: Box<dyn EvictionPolicy>,
    ) -> Result<Self> {
        if capacity == 0 {
            return Err(Error::InvalidArgument(
                "buffer pool capacity must be non-zero".into(),
            ));
        }
        let mut frames = Vec::with_capacity(capacity);
        for i in 0..capacity {
            frames.push(ParkingMutex::new(Frame::empty(i, page_size)?));
        }
        let page_table = (0..Self::NUM_SHARDS)
            .map(|_| RwLock::new(HashMap::new()))
            .collect();
        Ok(Self {
            page_size,
            disk,
            allocator,
            frames,
            page_table,
            eviction_lock: ParkingMutex::new(()),
            metrics,
            eviction_policy: policy,
        })
    }

    /// Return a reference to the pool's metrics collector.
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Return the name of the active eviction policy.
    pub fn eviction_policy_name(&self) -> &'static str {
        self.eviction_policy.name()
    }

    /// Return the configured page size.
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// Return the number of frames in the pool.
    pub fn capacity(&self) -> usize {
        self.frames.len()
    }

    /// Return a reference to the underlying paged file.
    pub(crate) fn disk(&self) -> &Arc<PagedFile> {
        &self.disk
    }

    /// Try to lock a frame without blocking.
    pub(crate) fn try_lock_frame(
        &self,
        frame_id: FrameId,
    ) -> Option<parking_lot::MutexGuard<'_, Frame>> {
        self.frames[frame_id].try_lock()
    }

    /// Fetch an existing page by id.  Returns a guard that logically pins the
    /// frame so the page cannot be evicted while the guard lives.
    pub fn fetch(self: &Arc<Self>, page_id: PageId) -> Result<PageGuard> {
        if page_id == NULL_PAGE_ID {
            return Err(Error::Corruption("fetch of null page id".into()));
        }
        loop {
            let shard = self.shard(page_id);
            let frame_id = {
                let table = self.page_table[shard].read();
                table.get(&page_id).copied()
            };
            if let Some(frame_id) = frame_id {
                let frame = self.frames[frame_id].lock();
                if frame.page_id == page_id {
                    let page = Arc::clone(&frame.page);
                    drop(frame);
                    return Ok(PageGuard::new(frame_id, page, Arc::clone(self)));
                }
                // Mapping changed between releasing the table lock and locking
                // the frame; retry.
                continue;
            }
            return Err(Error::NotFound(format!("page {page_id} not in pool")));
        }
    }

    /// Fetch a page from disk, installing it into the buffer pool if necessary.
    pub fn fetch_or_read(self: &Arc<Self>, page_id: PageId) -> Result<PageGuard> {
        if page_id == NULL_PAGE_ID {
            return Err(Error::Corruption("fetch of null page id".into()));
        }
        // Fast path: already cached.
        if let Ok(guard) = self.fetch(page_id) {
            return Ok(guard);
        }
        // Slow path: evict a frame and read from disk.
        let _evict = self.eviction_lock.lock();
        // Double-check after acquiring eviction lock.
        if let Ok(guard) = self.fetch(page_id) {
            return Ok(guard);
        }
        self.metrics.inc_cache_misses();
        let frame_id = self.evict()?;
        let bytes = self.disk.read_page(page_id)?;
        let page = Page::from_bytes(bytes)?;
        {
            let mut frame = self.frames[frame_id].lock();
            frame.page_id = page_id;
            frame.page = Arc::new(page);
            frame.dirty.store(false, Ordering::Relaxed);
            self.eviction_policy.record_install(page_id, &frame);
        }
        let shard = self.shard(page_id);
        let mut table = self.page_table[shard].write();
        table.insert(page_id, frame_id);
        drop(table);
        let frame = self.frames[frame_id].lock();
        let page = Arc::clone(&frame.page);
        drop(frame);
        Ok(PageGuard::new_installed(frame_id, page, Arc::clone(self)))
    }

    /// Fetch an existing page, or create a fresh empty page with the given id if
    /// it is not resident and cannot be read from disk.
    ///
    /// This is used during recovery for pages that were allocated before a crash
    /// but whose contents will be fully reconstructed by redo.
    pub fn fetch_or_create_page(self: &Arc<Self>, page_id: PageId) -> Result<PageGuard> {
        match self.fetch_or_read(page_id) {
            Ok(g) => Ok(g),
            Err(e) => match self.new_page_with_id(page_id) {
                Ok(g) => Ok(g),
                Err(_) => Err(e),
            },
        }
    }

    /// Allocate a new page id and install an empty page in the buffer pool.
    pub fn new_page(self: &Arc<Self>) -> Result<PageGuard> {
        let page_id = self.allocator.with_mut(|alloc| alloc.allocate());
        self.install_new_page(page_id)
    }

    /// Install an empty page with the given `page_id`, marking it as allocated.
    /// Used by recovery to resurrect pages that were allocated but not flushed.
    pub fn new_page_with_id(self: &Arc<Self>, page_id: PageId) -> Result<PageGuard> {
        self.allocator
            .with_mut(|alloc| alloc.allocate_specific(page_id));
        self.install_new_page(page_id)
    }

    /// Allocate a new page and run a read-only closure on it.
    ///
    /// The page pin is released as soon as the closure returns.
    pub fn with_new_page<R>(self: &Arc<Self>, f: impl FnOnce(&Page) -> R) -> Result<R> {
        let guard = self.new_page()?;
        Ok(guard.with(f))
    }

    /// Allocate a new page and run a mutating closure with the OLC write latch
    /// held.  The frame is marked dirty automatically on success.
    pub fn with_new_page_mut<R>(self: &Arc<Self>, f: impl FnOnce(&Page) -> Result<R>) -> Result<R> {
        let guard = self.new_page()?;
        guard.with_mut(f)
    }

    /// Fetch or read an existing page and run a read-only closure on it.
    ///
    /// The frame pin is released as soon as the closure returns, so this helper
    /// should be used for short, single-page reads where the caller does not
    /// need to keep the page resident.
    pub fn with_page<R>(
        self: &Arc<Self>,
        page_id: PageId,
        f: impl FnOnce(&Page) -> R,
    ) -> Result<R> {
        let guard = self.fetch_or_read(page_id)?;
        Ok(guard.with(f))
    }

    /// Fetch or read an existing page and run a mutating closure with the OLC
    /// write latch held.  The frame is marked dirty automatically on success.
    ///
    /// This is the preferred pattern for single-page writes: it bundles pin
    /// acquisition, exclusive latching, dirty marking, and latch release into
    /// one call, eliminating several classes of deadlocks and TOCTOU bugs.
    pub fn with_page_mut<R>(
        self: &Arc<Self>,
        page_id: PageId,
        f: impl FnOnce(&Page) -> Result<R>,
    ) -> Result<R> {
        let guard = self.fetch_or_read(page_id)?;
        guard.with_mut(f)
    }

    /// Fetch or create a page and run a mutating closure with the OLC write latch
    /// held.  The frame is marked dirty automatically on success.
    ///
    /// This is the recovery-time variant of `with_page_mut`: if the page is not
    /// resident and cannot be read from disk, a fresh empty page with the given
    /// id is installed so redo can reconstruct it.
    pub fn with_page_or_create_mut<R>(
        self: &Arc<Self>,
        page_id: PageId,
        f: impl FnOnce(&Page) -> Result<R>,
    ) -> Result<R> {
        let guard = self.fetch_or_create_page(page_id)?;
        guard.with_mut(f)
    }

    fn install_new_page(self: &Arc<Self>, page_id: PageId) -> Result<PageGuard> {
        let _evict = self.eviction_lock.lock();
        let frame_id = self.evict()?;
        {
            let mut frame = self.frames[frame_id].lock();
            frame.page_id = page_id;
            frame.page = Arc::new(Page::new(page_id, self.page_size)?);
            frame.dirty.store(true, Ordering::Relaxed);
            self.eviction_policy.record_install(page_id, &frame);
        }
        let shard = self.shard(page_id);
        let mut table = self.page_table[shard].write();
        table.insert(page_id, frame_id);
        drop(table);
        let frame = self.frames[frame_id].lock();
        let page = Arc::clone(&frame.page);
        drop(frame);
        Ok(PageGuard::new_installed(frame_id, page, Arc::clone(self)))
    }

    /// Flush a specific frame to disk if it is dirty.
    pub fn flush_frame(&self, frame_id: FrameId) -> Result<()> {
        let frame = self.frames[frame_id].lock();
        if frame.dirty.load(Ordering::Relaxed) && frame.page_id != NULL_PAGE_ID {
            self.disk.write_page(frame.page_id, &frame.page.data())?;
            frame.dirty.store(false, Ordering::Relaxed);
            self.metrics.inc_page_flushes();
        }
        Ok(())
    }

    /// Flush all dirty frames to disk.
    pub fn flush_all(&self) -> Result<()> {
        for frame_id in 0..self.frames.len() {
            self.flush_frame(frame_id)?;
        }
        Ok(())
    }

    /// fsync the underlying page file and its directory.
    pub(crate) fn sync_disk(&self) -> Result<()> {
        self.disk.sync()
    }

    /// Flush every dirty frame that is not currently pinned.
    ///
    /// This is the core operation of the background page cleaner. Pinned frames
    /// are skipped because they may still be mutated by the holder of the pin.
    pub fn flush_dirty_unpinned(&self) -> Result<()> {
        for frame_id in 0..self.frames.len() {
            let frame = match self.frames[frame_id].try_lock() {
                Some(g) => g,
                None => continue,
            };
            if frame.page_id != NULL_PAGE_ID
                && frame.dirty.load(Ordering::Relaxed)
                && frame.pin_count.load(Ordering::Acquire) == 0
            {
                self.disk.write_page(frame.page_id, &frame.page.data())?;
                frame.dirty.store(false, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Return whether the frame holding `page_id` is currently dirty.
    ///
    /// Returns `Err` if the page is not resident in the pool.
    #[cfg(test)]
    pub fn is_frame_dirty(&self, page_id: PageId) -> Result<bool> {
        let shard = self.shard(page_id);
        let frame_id = {
            let table = self.page_table[shard].read();
            table.get(&page_id).copied()
        };
        if let Some(frame_id) = frame_id {
            let frame = self.frames[frame_id].lock();
            if frame.page_id == page_id {
                return Ok(frame.dirty.load(Ordering::Relaxed));
            }
        }
        Err(Error::NotFound(format!(
            "page {page_id} not resident in pool"
        )))
    }

    /// Mark the frame containing `page_id` dirty.
    ///
    /// This is intended for callers that hold the page's OLC write latch but
    /// do not have a mutable borrow of the `PageGuard`.
    pub fn mark_dirty(&self, page_id: PageId) -> Result<()> {
        let shard = self.shard(page_id);
        let frame_id = {
            let table = self.page_table[shard].read();
            table.get(&page_id).copied()
        };
        if let Some(frame_id) = frame_id {
            let frame = self.frames[frame_id].lock();
            if frame.page_id == page_id {
                frame.dirty.store(true, Ordering::Relaxed);
                return Ok(());
            }
        }
        Err(Error::Corruption(format!(
            "mark_dirty called for page {page_id} not resident in pool"
        )))
    }

    /// Remove a page from the pool and return its id to the allocator freelist.
    ///
    /// The caller must ensure the page is no longer reachable from the tree
    /// before calling this method.
    pub fn free_page(self: &Arc<Self>, page_id: PageId) -> Result<()> {
        if page_id == NULL_PAGE_ID {
            return Ok(());
        }
        let _evict = self.eviction_lock.lock();
        let shard = self.shard(page_id);
        let frame_id = {
            let table = self.page_table[shard].read();
            table.get(&page_id).copied()
        };
        if let Some(frame_id) = frame_id {
            let mut frame = self.frames[frame_id].lock();
            if frame.page_id == page_id {
                if frame.pin_count.load(Ordering::Acquire) > 0 {
                    return Err(Error::Corruption(format!(
                        "free_page({page_id}) called while frame is still pinned"
                    )));
                }
                frame.page_id = NULL_PAGE_ID;
                frame.page = Arc::new(Page::new(PageId::new(frame_id as u64), self.page_size)?);
                frame.dirty.store(false, Ordering::Relaxed);
                frame.ref_bit.store(false, Ordering::Relaxed);
                frame.set_hot(false);
                frame.set_cold_test(false);
                let mut table = self.page_table[shard].write();
                table.remove(&page_id);
            }
        }
        self.allocator.with_mut(|alloc| alloc.free(page_id));
        Ok(())
    }

    /// Run the eviction policy and return an empty frame id.
    ///
    /// The caller must hold `eviction_lock`.
    fn evict(&self) -> Result<FrameId> {
        let hand = self
            .eviction_policy
            .select_victim(&self.frames)
            .ok_or_else(|| {
                Error::Corruption(format!(
                    "{} eviction could not find a victim frame",
                    self.eviction_policy.name()
                ))
            })?;

        let mut frame = self.frames[hand].lock();
        self.eviction_policy.record_eviction(hand, &frame);
        self.metrics.inc_evictions();

        if frame.page_id != NULL_PAGE_ID {
            if frame.dirty.load(Ordering::Relaxed) {
                self.disk.write_page(frame.page_id, &frame.page.data())?;
                self.metrics.inc_page_flushes();
            }
            let shard = self.shard(frame.page_id);
            let mut table = self.page_table[shard].write();
            table.remove(&frame.page_id);
            frame.page_id = NULL_PAGE_ID;
        }
        frame.dirty.store(false, Ordering::Relaxed);
        frame.ref_bit.store(false, Ordering::Relaxed);
        frame.set_hot(false);
        frame.set_cold_test(false);
        Ok(hand)
    }

    fn shard(&self, page_id: PageId) -> usize {
        (page_id.get() as usize) % Self::NUM_SHARDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool(capacity: usize) -> (Arc<BufferPool>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool = Arc::new(BufferPool::new(capacity, 512, disk, alloc).unwrap());
        (pool, dir)
    }

    #[test]
    fn new_page_allocates_and_pins() {
        let (pool, _dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        assert_ne!(guard.page().id, NULL_PAGE_ID);
    }

    #[test]
    fn fetch_returns_cached_page() {
        let (pool, _dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        let id = guard.page().id;
        guard
            .page()
            .insert(b"k", &crate::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();
        drop(guard);

        let guard = pool.fetch(id).unwrap();
        let cell = guard.page().get(b"k").unwrap().unwrap();
        assert_eq!(
            cell.value.as_value_kind(),
            crate::slot::ValueKind::Inline(b"v")
        );
    }

    #[test]
    fn fetch_missing_page_fails() {
        let (pool, _dir) = make_pool(4);
        assert!(pool.fetch(PageId::new(99)).is_err());
    }

    #[test]
    fn eviction_reuses_unpinned_frames() {
        let (pool, _dir) = make_pool(2);
        let g1 = pool.new_page().unwrap();
        let id1 = g1.page().id;
        drop(g1);

        let g2 = pool.new_page().unwrap();
        let id2 = g2.page().id;
        drop(g2);

        // Allocate a third page; one of the first two must be evicted.
        let g3 = pool.new_page().unwrap();
        let id3 = g3.page().id;
        drop(g3);

        assert!(pool.fetch(id1).is_err() || pool.fetch(id2).is_err());
        assert!(pool.fetch(id3).is_ok());
    }

    #[test]
    fn pinned_frames_are_not_evicted() {
        let (pool, _dir) = make_pool(2);
        let _g1 = pool.new_page().unwrap();
        let _g2 = pool.new_page().unwrap();

        // Both frames are pinned, so allocating a third page should fail.
        assert!(pool.new_page().is_err());
    }

    #[test]
    fn flush_all_persists_pages() {
        let (pool, dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        let id = guard.page().id;
        guard
            .page()
            .insert(b"k", &crate::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();
        drop(guard);

        pool.flush_all().unwrap();

        // Open a fresh pool over the same file and read the page.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let pool2 = Arc::new(BufferPool::new(4, 512, disk, alloc).unwrap());
        let guard = pool2.fetch_or_read(id).unwrap();
        let cell = guard.page().get(b"k").unwrap().unwrap();
        assert_eq!(
            cell.value.as_value_kind(),
            crate::slot::ValueKind::Inline(b"v")
        );
    }

    #[test]
    fn with_page_reads_and_releases_pin() {
        let (pool, _dir) = make_pool(2);
        let id = pool
            .with_new_page_mut(|page| {
                page.insert(b"k", &crate::slot::ValueKind::Inline(b"v"))?;
                Ok(page.id)
            })
            .unwrap();

        // The pin from with_new_page_mut should be released, so the frame is
        // evictable.  Reading back through with_page should still work.
        let value = pool
            .with_page(id, |page| {
                page.get(b"k")
                    .unwrap()
                    .map(|c| c.value.as_value_kind().into_owned())
            })
            .unwrap();
        assert_eq!(value, Some(crate::slot::OwnedValue::Inline(b"v".to_vec())));
    }

    #[test]
    fn with_page_mut_marks_dirty_and_writes() {
        let (pool, _dir) = make_pool(4);
        let id = pool
            .with_new_page_mut(|page| {
                page.insert(b"k", &crate::slot::ValueKind::Inline(b"v1"))?;
                Ok(page.id)
            })
            .unwrap();

        pool.with_page_mut(id, |page| {
            page.insert(b"k", &crate::slot::ValueKind::Inline(b"v2"))?;
            Ok(())
        })
        .unwrap();

        assert!(pool.is_frame_dirty(id).unwrap());
        let value = pool
            .with_page(id, |page| {
                page.get(b"k")
                    .unwrap()
                    .map(|c| c.value.as_value_kind().into_owned())
            })
            .unwrap();
        assert_eq!(value, Some(crate::slot::OwnedValue::Inline(b"v2".to_vec())));
    }

    #[test]
    fn with_page_mut_does_not_mark_dirty_on_error() {
        let (pool, _dir) = make_pool(4);
        let id = pool
            .with_new_page_mut(|page| {
                page.insert(b"k", &crate::slot::ValueKind::Inline(b"v"))?;
                Ok(page.id)
            })
            .unwrap();

        // Flush the page so it is clean.
        pool.flush_all().unwrap();
        assert!(!pool.is_frame_dirty(id).unwrap());

        // Return an error from the closure: the frame should stay clean.
        let result: Result<()> = pool.with_page_mut(id, |_page| {
            Err(Error::Corruption("intentional test error".into()))
        });
        assert!(result.is_err());
        assert!(!pool.is_frame_dirty(id).unwrap());
    }

    #[test]
    fn guard_with_mut_marks_dirty_on_success() {
        let (pool, _dir) = make_pool(4);
        let guard = pool.new_page().unwrap();
        let id = guard.page().id;
        guard
            .with_mut(|page| {
                page.insert(b"k", &crate::slot::ValueKind::Inline(b"v"))?;
                Ok(())
            })
            .unwrap();
        assert!(pool.is_frame_dirty(id).unwrap());
    }

    fn make_pool_with_policy(
        capacity: usize,
        policy: Box<dyn EvictionPolicy>,
    ) -> (Arc<BufferPool>, tempfile::TempDir, Arc<Metrics>) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(SyncMutex::new(PageAllocator::new(PageId::new(1))));
        let metrics = Arc::new(Metrics::new());
        let pool = Arc::new(
            BufferPool::with_policy(capacity, 512, disk, alloc, Arc::clone(&metrics), policy)
                .unwrap(),
        );
        (pool, dir, metrics)
    }

    #[test]
    fn simple_clock_policy_evicts_unpinned_frames() {
        let (pool, _dir, _) =
            make_pool_with_policy(2, Box::new(crate::eviction::SimpleClock::new()));
        let g1 = pool.new_page().unwrap();
        let id1 = g1.page().id;
        drop(g1);
        let g2 = pool.new_page().unwrap();
        let id2 = g2.page().id;
        drop(g2);
        let g3 = pool.new_page().unwrap();
        let id3 = g3.page().id;
        drop(g3);
        assert!(pool.fetch(id1).is_err() || pool.fetch(id2).is_err());
        assert!(pool.fetch(id3).is_ok());
    }

    #[test]
    fn clock_pro_policy_evicts_unpinned_frames() {
        let (pool, _dir, _) = make_pool_with_policy(2, Box::new(ClockPro::new(2)));
        let g1 = pool.new_page().unwrap();
        let id1 = g1.page().id;
        drop(g1);
        let g2 = pool.new_page().unwrap();
        let id2 = g2.page().id;
        drop(g2);
        let g3 = pool.new_page().unwrap();
        let id3 = g3.page().id;
        drop(g3);
        assert!(pool.fetch(id1).is_err() || pool.fetch(id2).is_err());
        assert!(pool.fetch(id3).is_ok());
    }

    #[test]
    fn clock_pro_prefers_frequently_accessed_pages() {
        let (pool, _dir, _) = make_pool_with_policy(4, Box::new(ClockPro::new(4)));
        let guards: Vec<_> = (0..4).map(|_| pool.new_page().unwrap()).collect();
        let ids: Vec<_> = guards.iter().map(|g| g.page().id).collect();
        // Make the first two pages hot.
        for _ in 0..5 {
            let _ = pool.fetch(ids[0]).unwrap();
            let _ = pool.fetch(ids[1]).unwrap();
        }
        drop(guards);
        // Evict by allocating more pages.  Re-touch the hot pages between each
        // eviction so the hot hand sees them as still referenced and does not
        // demote them.
        for _ in 0..4 {
            let _ = pool.new_page().unwrap();
            let _ = pool.fetch(ids[0]).unwrap();
            let _ = pool.fetch(ids[1]).unwrap();
        }
        assert!(pool.fetch(ids[0]).is_ok(), "hot page 0 should survive");
        assert!(pool.fetch(ids[1]).is_ok(), "hot page 1 should survive");
        // At least one of the untouched cold pages should have been evicted.
        assert!(
            pool.fetch(ids[2]).is_err() || pool.fetch(ids[3]).is_err(),
            "a cold page should have been evicted"
        );
    }

    #[test]
    fn eviction_increments_counter() {
        let (pool, _dir, metrics) = make_pool_with_policy(2, Box::new(ClockPro::new(2)));
        let g1 = pool.new_page().unwrap();
        drop(g1);
        let g2 = pool.new_page().unwrap();
        drop(g2);
        let before = metrics.snapshot().evictions;
        let _g3 = pool.new_page().unwrap();
        assert!(
            metrics.snapshot().evictions > before,
            "eviction counter should increase"
        );
    }
}
