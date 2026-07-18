//! Frame-based buffer pool.
//!
//! The buffer pool caches on-disk pages in memory, tracks dirty pages, and
//! evicts cold pages using a CLOCK policy.  Each frame holds an `Arc<Page>`,
//! so threads can release the frame metadata lock while retaining a logical
//! pin on the page.  The per-page Optimistic Lock Coupling (OLC) latch in
//! `Page::latch_word` serialises concurrent access to the page contents.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use parking_lot::{Mutex as ParkingMutex, RwLock};

use crate::error::{Error, Result};
use crate::v2::disk::PagedFile;
use crate::v2::page::{NULL_PAGE_ID, Page, PageId};
use crate::v2::space::PageAllocator;

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
}

impl Frame {
    fn empty(frame_id: FrameId, page_size: usize) -> Result<Self> {
        Ok(Self {
            page_id: NULL_PAGE_ID,
            page: Arc::new(Page::new(frame_id as PageId, page_size)?),
            pin_count: AtomicU32::new(0),
            dirty: AtomicBool::new(false),
            ref_bit: AtomicBool::new(false),
        })
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
    fn new(frame_id: FrameId, page: Arc<Page>, pool: Arc<BufferPool>) -> Self {
        pool.frames[frame_id]
            .lock()
            .pin_count
            .fetch_add(1, Ordering::Relaxed);
        pool.frames[frame_id].lock().ref_bit.store(true, Ordering::Relaxed);
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
    allocator: Arc<Mutex<PageAllocator>>,
    frames: Vec<ParkingMutex<Frame>>,
    /// Sharded page table: `PageId -> FrameId`.
    page_table: Vec<RwLock<HashMap<PageId, FrameId>>>,
    /// Serializes eviction and new-page installation to avoid deadlocks with
    /// the sharded page table.
    eviction_lock: ParkingMutex<()>,
    clock_hand: AtomicUsize,
}

impl BufferPool {
    const NUM_SHARDS: usize = 64;

    /// Create a buffer pool with `capacity` frames.
    pub fn new(
        capacity: usize,
        page_size: usize,
        disk: Arc<PagedFile>,
        allocator: Arc<Mutex<PageAllocator>>,
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
            clock_hand: AtomicUsize::new(0),
        })
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
        let frame_id = self.evict()?;
        let bytes = self.disk.read_page(page_id)?;
        let page = Page::from_bytes(bytes)?;
        {
            let mut frame = self.frames[frame_id].lock();
            frame.page_id = page_id;
            frame.page = Arc::new(page);
            frame.dirty.store(false, Ordering::Relaxed);
            frame.ref_bit.store(true, Ordering::Relaxed);
        }
        let shard = self.shard(page_id);
        let mut table = self.page_table[shard].write();
        table.insert(page_id, frame_id);
        drop(table);
        let frame = self.frames[frame_id].lock();
        let page = Arc::clone(&frame.page);
        drop(frame);
        Ok(PageGuard::new(frame_id, page, Arc::clone(self)))
    }

    /// Allocate a new page id and install an empty page in the buffer pool.
    pub fn new_page(self: &Arc<Self>) -> Result<PageGuard> {
        let page_id = {
            let mut alloc = self
                .allocator
                .lock()
                .map_err(|_| Error::Corruption("page allocator mutex poisoned".into()))?;
            alloc.allocate()
        };
        let _evict = self.eviction_lock.lock();
        let frame_id = self.evict()?;
        {
            let mut frame = self.frames[frame_id].lock();
            frame.page_id = page_id;
            frame.page = Arc::new(Page::new(page_id, self.page_size)?);
            frame.dirty.store(true, Ordering::Relaxed);
            frame.ref_bit.store(true, Ordering::Relaxed);
        }
        let shard = self.shard(page_id);
        let mut table = self.page_table[shard].write();
        table.insert(page_id, frame_id);
        drop(table);
        let frame = self.frames[frame_id].lock();
        let page = Arc::clone(&frame.page);
        drop(frame);
        Ok(PageGuard::new(frame_id, page, Arc::clone(self)))
    }

    /// Flush a specific frame to disk if it is dirty.
    pub fn flush_frame(&self, frame_id: FrameId) -> Result<()> {
        let frame = self.frames[frame_id].lock();
        if frame.dirty.load(Ordering::Relaxed) && frame.page_id != NULL_PAGE_ID {
            self.disk.write_page(frame.page_id, frame.page.data())?;
            frame.dirty.store(false, Ordering::Relaxed);
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
                frame.page = Arc::new(Page::new(frame_id as PageId, self.page_size)?);
                frame.dirty.store(false, Ordering::Relaxed);
                frame.ref_bit.store(false, Ordering::Relaxed);
                let mut table = self.page_table[shard].write();
                table.remove(&page_id);
            }
        }
        let mut alloc = self
            .allocator
            .lock()
            .map_err(|_| Error::Corruption("page allocator mutex poisoned".into()))?;
        alloc.free(page_id);
        Ok(())
    }

    /// Run one pass of the CLOCK algorithm and return an empty frame id.
    ///
    /// The caller must hold `eviction_lock`.
    fn evict(&self) -> Result<FrameId> {
        let capacity = self.frames.len();
        for _ in 0..capacity * 4 {
            let hand = self.clock_hand.fetch_add(1, Ordering::Relaxed) % capacity;
            let mut frame = match self.frames[hand].try_lock() {
                Some(g) => g,
                None => continue,
            };
            if frame.pin_count.load(Ordering::Acquire) > 0 {
                continue;
            }
            if frame.ref_bit.load(Ordering::Relaxed) {
                frame.ref_bit.store(false, Ordering::Relaxed);
                continue;
            }
            // Found a victim.
            if frame.page_id != NULL_PAGE_ID {
                if frame.dirty.load(Ordering::Relaxed) {
                    self.disk.write_page(frame.page_id, frame.page.data())?;
                }
                let shard = self.shard(frame.page_id);
                let mut table = self.page_table[shard].write();
                table.remove(&frame.page_id);
                frame.page_id = NULL_PAGE_ID;
            }
            frame.dirty.store(false, Ordering::Relaxed);
            frame.ref_bit.store(false, Ordering::Relaxed);
            return Ok(hand);
        }
        Err(Error::Corruption(
            "CLOCK eviction could not find a victim frame".into(),
        ))
    }

    fn shard(&self, page_id: PageId) -> usize {
        (page_id as usize) % Self::NUM_SHARDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pool(capacity: usize) -> (Arc<BufferPool>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(Mutex::new(PageAllocator::new(1)));
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
            .insert(b"k", &crate::v2::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();
        drop(guard);

        let guard = pool.fetch(id).unwrap();
        let cell = guard.page().get(b"k").unwrap().unwrap();
        assert_eq!(cell.value, crate::v2::slot::ValueKind::Inline(b"v"));
    }

    #[test]
    fn fetch_missing_page_fails() {
        let (pool, _dir) = make_pool(4);
        assert!(pool.fetch(99).is_err());
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
            .insert(b"k", &crate::v2::slot::ValueKind::Inline(b"v"))
            .unwrap();
        guard.mark_dirty();
        drop(guard);

        pool.flush_all().unwrap();

        // Open a fresh pool over the same file and read the page.
        let disk = Arc::new(PagedFile::open(dir.path().join("pages.dat"), 512).unwrap());
        let alloc = Arc::new(Mutex::new(PageAllocator::new(1)));
        let pool2 = Arc::new(BufferPool::new(4, 512, disk, alloc).unwrap());
        let guard = pool2.fetch_or_read(id).unwrap();
        let cell = guard.page().get(b"k").unwrap().unwrap();
        assert_eq!(cell.value, crate::v2::slot::ValueKind::Inline(b"v"));
    }
}
