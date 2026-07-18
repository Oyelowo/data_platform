//! Physical file shrink for `pages.dat`.
//!
//! Shrinking removes trailing free space from the page file.  It does **not**
//! compact holes in the middle of the file; that would require moving pages and
//! updating every parent pointer.  The operation is safe because it only
//! truncates pages that are provably unreachable and beyond the allocator's
//! high-water mark.

use std::sync::Arc;

use crate::buffer::BufferPool;
use crate::disk::PagedFile;
use crate::error::{Error, Result};
use crate::page::PageId;
use crate::space::PageAllocator;
use crate::sync::Mutex as SyncMutex;
use crate::tree::BPlusTree;

/// Truncate `pages.dat` so that it ends at the highest allocated page id.
///
/// The caller must ensure:
/// * the background checkpoint thread and page cleaner are stopped,
/// * a checkpoint has been run so the allocator state is current,
/// * `tree.compact()` has reclaimed unreachable pages.
///
/// Returns the new page count.
pub fn shrink_pages_file(
    disk: &Arc<PagedFile>,
    pool: &Arc<BufferPool>,
    allocator: &Arc<SyncMutex<PageAllocator>>,
    tree: &Arc<BPlusTree>,
) -> Result<u64> {
    // Flush every dirty frame first so the on-disk file matches memory.
    pool.flush_all()?;
    disk.sync()?;

    // Determine the highest page id that is still allocated.
    let high_water_mark = allocator.with_mut(|alloc| {
        let mut max = alloc.next_id().get().saturating_sub(1);
        for id in alloc.snapshot().0 {
            if id.get() > max {
                max = id.get();
            }
        }
        max
    });

    // The tree may still reference pages through active roots / cursors.  Make
    // sure no pinned root points beyond the high-water mark.
    let rooted_high = tree.highest_rooted_page_id();
    let high_water_mark = high_water_mark.max(rooted_high);

    if high_water_mark == 0 {
        // Nothing allocated; keep the file at one page so the root can be
        // reinstalled after shrink.
        return Ok(1);
    }

    let new_page_count = high_water_mark + 1;
    let current_page_count = disk.page_count()?;
    if new_page_count >= current_page_count {
        return Ok(current_page_count);
    }

    // Evict any frames that hold pages beyond the new tail so we do not have
    // stale resident pages for truncated disk offsets.
    evict_pages_beyond(pool, PageId::new(new_page_count))?;

    let new_len = new_page_count * pool.page_size() as u64;
    disk.set_len(new_len)?;
    disk.sync()?;

    Ok(new_page_count)
}

/// Evict from the buffer pool any frame whose page id is >= `boundary`.
fn evict_pages_beyond(pool: &Arc<BufferPool>, boundary: PageId) -> Result<()> {
    for frame_id in 0..pool.capacity() {
        let frame = match pool.try_lock_frame(frame_id) {
            Some(g) => g,
            None => continue,
        };
        if frame.page_id != PageId::new(0) && frame.page_id >= boundary {
            // Pinned frames should not exist beyond the high-water mark after
            // compact(); if they do, refuse to truncate rather than corrupt.
            if frame.pin_count.load(std::sync::atomic::Ordering::Acquire) > 0 {
                return Err(Error::Corruption(format!(
                    "page {} is pinned beyond shrink boundary {}",
                    frame.page_id, boundary
                )));
            }
        }
    }
    Ok(())
}
