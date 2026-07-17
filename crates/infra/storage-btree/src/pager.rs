//! Page file management and in-memory cache.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Weak;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::node::{Value, build_overflow_body, decode_overflow_page};
use crate::page::{NULL_PAGE_ID, Page, PageId};

const PAGE_FILE_NAME: &str = "pages.dat";

/// Sharded cache for immutable pages.
struct PageCache {
    shards: Vec<RwLock<lru::LruCache<PageId, std::sync::Arc<Page>>>>,
}

impl PageCache {
    fn new(capacity: usize) -> Self {
        // Use a fixed shard count that is a power of two for cheap indexing.
        const NUM_SHARDS: usize = 64;
        let per_shard = (capacity / NUM_SHARDS).max(1);
        let shards = (0..NUM_SHARDS)
            .map(|_| {
                RwLock::new(lru::LruCache::new(
                    NonZeroUsize::new(per_shard).unwrap_or(NonZeroUsize::MIN),
                ))
            })
            .collect();
        Self { shards }
    }

    fn shard_index(id: PageId) -> usize {
        (id as usize) & (64 - 1)
    }

    fn get(&self, id: PageId) -> Option<std::sync::Arc<Page>> {
        let shard = &self.shards[Self::shard_index(id)];
        // `get` (not `peek`) updates LRU recency so read-heavy workloads do not
        // evict hot pages. The LRU shard is write-locked only briefly.
        shard.write().get(&id).cloned()
    }

    fn put(&self, id: PageId, page: std::sync::Arc<Page>) {
        let shard = &self.shards[Self::shard_index(id)];
        shard.write().put(id, page);
    }
}

/// Manages the on-disk page file and an in-memory cache of immutable pages.
pub(crate) struct Pager {
    page_size: usize,
    dir: PathBuf,
    file: Mutex<File>,
    cache: PageCache,
    /// Next page id to allocate if the freelist is empty.
    next_page_id: AtomicU64,
    /// Reusable page slots that were never published.
    freelist: Mutex<Vec<PageId>>,
    /// Page ids that were retired from the tree. A retired id may be reused
    /// only when no reader holds an `Arc<Page>` for it.
    retired: Mutex<HashMap<PageId, Weak<Page>>>,
}

impl Pager {
    /// Open or create the page file at `dir/PAGE_FILE_NAME`.
    pub fn open(dir: impl AsRef<Path>, page_size: usize, cache_size: usize) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(PAGE_FILE_NAME);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        let file_len = file.metadata()?.len();
        let next_page_id = if file_len == 0 {
            // Reserve page id 0 for NULL_PAGE_ID.
            1
        } else {
            (file_len / page_size as u64).max(1)
        };

        // A cache_size of zero means "unlimited", but the LRU implementation
        // requires a bounded capacity. We use a generous cap that still fits
        // comfortably in memory.
        const UNLIMITED_CAP: usize = 1 << 24;
        let cache_capacity = if cache_size == 0 {
            UNLIMITED_CAP
        } else {
            (cache_size / page_size).max(1)
        };

        Ok(Self {
            page_size,
            dir,
            file: Mutex::new(file),
            cache: PageCache::new(cache_capacity),
            next_page_id: AtomicU64::new(next_page_id),
            freelist: Mutex::new(Vec::new()),
            retired: Mutex::new(HashMap::new()),
        })
    }

    /// Allocate a fresh page id, reusing a safe retired or freelisted slot if
    /// one is available.
    pub fn allocate(&self) -> PageId {
        // Prefer never-published freelist slots.
        if let Some(id) = self.freelist.lock().pop() {
            return id;
        }

        // Retired page ids are intentionally NOT reused here. A retired page may
        // still be reachable from an older tree snapshot that is not currently
        // cached, so reusing its id would corrupt readers that later traverse
        // that snapshot. Reclamation of retired pages is deferred to a future
        // compaction / garbage-collection phase that walks reachable pages.
        self.next_page_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Retire a page id. The id will not be reused until no reader holds the
    /// page in cache.
    pub fn retire(&self, id: PageId) {
        if id == NULL_PAGE_ID {
            return;
        }
        // Capture a weak reference before removing the page from the cache so
        // that an in-flight reader holding an Arc keeps the id alive.
        let weak = if let Some(page) = self.cache.get(id) {
            Arc::downgrade(&page)
        } else {
            Weak::new()
        };

        // Remove from the cache so new readers do not pick up a stale copy.
        let shard = &self.cache.shards[PageCache::shard_index(id)];
        shard.write().pop(&id);

        self.retired.lock().insert(id, weak);
    }

    /// Retire an entire overflow chain so its page ids are not reused while any
    /// reader may still be reading the large value.
    ///
    /// The chain is read into the page cache before retiring so that active
    /// readers hold live `Arc<Page>` references and the weak retirement refs
    /// are non-empty.
    pub fn retire_overflow(&self, head: PageId) {
        let mut current = head;
        let mut to_retire = Vec::new();
        let mut visited = std::collections::HashSet::new();
        while current != NULL_PAGE_ID {
            if !visited.insert(current) {
                break;
            }
            to_retire.push(current);
            let next = self
                .read(current)
                .ok()
                .and_then(|p| decode_overflow_page(&p).ok().map(|(n, _)| n))
                .unwrap_or(NULL_PAGE_ID);
            current = next;
        }
        for id in to_retire {
            self.retire(id);
        }
    }

    /// Read a page by id, using the cache on hit and loading from disk on miss.
    pub fn read(&self, id: PageId) -> Result<std::sync::Arc<Page>> {
        if id == NULL_PAGE_ID {
            return Err(Error::Corruption("read of null page id".into()));
        }
        if let Some(page) = self.cache.get(id) {
            return Ok(page);
        }
        let page = self.load(id)?;
        let shared = std::sync::Arc::new(page);
        self.cache.put(id, std::sync::Arc::clone(&shared));
        Ok(shared)
    }

    /// Write a page to disk. The page id is used to determine the file offset.
    pub fn write(&self, page: &Page) -> Result<()> {
        let offset = page.id * self.page_size as u64;
        let mut file = self.file.lock();
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&page.data)?;
        file.flush()?;
        Ok(())
    }

    /// Ensure all writes are durably on disk, including the directory entry.
    pub fn sync(&self) -> Result<()> {
        let file = self.file.lock();
        file.sync_all()?;
        drop(file);
        // Sync the directory so the file's existence and size are durable.
        let dir = File::open(&self.dir)?;
        dir.sync_all()?;
        Ok(())
    }

    fn load(&self, id: PageId) -> Result<Page> {
        let offset = id * self.page_size as u64;
        let mut file = self.file.lock();
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; self.page_size];
        match file.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(Error::Corruption(format!(
                    "page {id} read past end of file"
                )));
            }
            Err(e) => return Err(e.into()),
        }
        Page::from_bytes(id, Bytes::from(buf), self.page_size)
    }

    /// Return the current freelist and next page id for checkpointing.
    pub fn freelist_snapshot(&self) -> (Vec<PageId>, PageId) {
        (
            self.freelist.lock().clone(),
            self.next_page_id.load(Ordering::SeqCst),
        )
    }

    /// Restore the freelist and next page id during recovery.
    pub fn restore_freelist(&self, freelist: Vec<PageId>, next_page_id: PageId) {
        *self.freelist.lock() = freelist;
        self.next_page_id.store(next_page_id, Ordering::SeqCst);
    }

    /// Write a large value as a chain of overflow pages and return the head
    /// page id.
    pub fn write_overflow(&self, value: &[u8]) -> Result<PageId> {
        let payload_capacity = (self.page_size - 4)
            .saturating_sub(crate::page::PageHeader::SIZE + 8 + 4)
            .max(1);
        let total_fragments = value.len().div_ceil(payload_capacity);
        if total_fragments == 0 {
            // Empty overflow value: still allocate one page.
            let id = self.allocate();
            let body = build_overflow_body(NULL_PAGE_ID, &[], self.page_size)?;
            let page = Page::build(id, body, self.page_size)?;
            self.write(&page)?;
            return Ok(id);
        }

        let ids: Vec<PageId> = (0..total_fragments).map(|_| self.allocate()).collect();
        for (idx, id) in ids.iter().copied().enumerate().rev() {
            let start = idx * payload_capacity;
            let end = ((idx + 1) * payload_capacity).min(value.len());
            let next = if idx + 1 < ids.len() {
                ids[idx + 1]
            } else {
                NULL_PAGE_ID
            };
            let body = build_overflow_body(next, &value[start..end], self.page_size)?;
            let page = Page::build(id, body, self.page_size)?;
            self.write(&page)?;
        }
        Ok(ids[0])
    }

    /// Read a large value from a chain of overflow pages starting at `head`.
    pub fn read_overflow(&self, head: PageId) -> Result<Bytes> {
        let mut out = Vec::new();
        let mut current = head;
        let mut visited = std::collections::HashSet::new();
        while current != NULL_PAGE_ID {
            if !visited.insert(current) {
                return Err(Error::Corruption("overflow cycle detected".into()));
            }
            let page = self.read(current)?;
            let (next, payload) = decode_overflow_page(&page)?;
            out.extend_from_slice(&payload);
            current = next;
        }
        Ok(Bytes::from(out))
    }

    /// Resolve a value: inline values are cloned, overflow values are read.
    pub fn resolve_value(&self, value: &Value) -> Result<Bytes> {
        match value {
            Value::Inline(bytes) => Ok(bytes.clone()),
            Value::Overflow(head) => self.read_overflow(*head),
        }
    }

    /// Validate an overflow chain without materialising the full value.
    ///
    /// Returns the number of pages in the chain. Detects cycles and malformed
    /// overflow pages.
    pub fn validate_overflow(&self, head: PageId) -> Result<usize> {
        let mut current = head;
        let mut visited = std::collections::HashSet::new();
        let mut count = 0;
        while current != NULL_PAGE_ID {
            if !visited.insert(current) {
                return Err(Error::Corruption("overflow cycle detected".into()));
            }
            let page = self.read(current)?;
            let (next, _) = decode_overflow_page(&page)?;
            current = next;
            count += 1;
        }
        Ok(count)
    }

    /// Approximate number of bytes held by the page cache.
    pub fn approx_memory_bytes(&self) -> u64 {
        let entries: usize = self
            .cache
            .shards
            .iter()
            .map(|shard| shard.read().len())
            .sum();
        (entries * self.page_size) as u64
    }

    /// Count of retired page ids that are waiting for reclamation.
    pub fn retired_count(&self) -> usize {
        self.retired.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_and_write_page() {
        let dir = tempfile::tempdir().unwrap();
        let pager = Pager::open(dir.path(), 4096, 0).unwrap();

        let id = pager.allocate();
        assert_eq!(id, 1);

        let mut body = bytes::BytesMut::zeroed(4092);
        body[0..4].copy_from_slice(&crate::page::PAGE_MAGIC.to_le_bytes());
        body[4..6].copy_from_slice(&crate::page::PAGE_FORMAT_VERSION.to_le_bytes());
        body[6] = crate::page::PageType::Leaf.encode();
        let page = Page::build(id, body, 4096).unwrap();
        pager.write(&page).unwrap();

        let read = pager.read(id).unwrap();
        assert_eq!(read.id, id);
        assert_eq!(read.data.len(), 4096);
    }

    #[test]
    fn overflow_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pager = Pager::open(dir.path(), 512, 0).unwrap();
        let value = vec![0xABu8; 4096];
        let head = pager.write_overflow(&value).unwrap();
        let read = pager.read_overflow(head).unwrap();
        assert_eq!(read.as_ref(), value.as_slice());
    }

    #[test]
    fn overflow_cycle_detected() {
        let dir = tempfile::tempdir().unwrap();
        let pager = Pager::open(dir.path(), 512, 0).unwrap();
        let id = pager.allocate();
        let body = build_overflow_body(id, b"x", 512).unwrap();
        let page = Page::build(id, body, 512).unwrap();
        pager.write(&page).unwrap();
        let result = pager.read_overflow(id);
        assert!(result.is_err());
    }
}
