//! Buffer-pool eviction policies.
//!
//! The default policy is CLOCK-Pro, an approximate LIRS replacement algorithm
//! that keeps CLOCK's lock-free O(1) access path while adding scan resistance
//! and frequency awareness through hot/cold classification and non-resident
//! "ghost" metadata.  See `PHASE11_DESIGN.md` for the research rationale.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use crate::buffer::{Frame, FrameId};
use crate::page::{NULL_PAGE_ID, PageId};

/// Maximum fraction of the buffer pool that may be used for non-resident
/// metadata entries (ghost pages).
const MAX_GHOST_RATIO: usize = 2;

/// Eviction-policy interface used by [`BufferPool`](crate::buffer::BufferPool).
///
/// Implementations must be thread-safe because `record_access` is called on the
/// hot path while `select_victim` runs under the pool's eviction lock.
pub trait EvictionPolicy: Debug + Send + Sync + 'static {
    /// Called when a resident frame is accessed (cache hit).
    fn record_access(&self, frame_id: FrameId, frame: &Frame);

    /// Called when a page is installed into a frame after a cache miss.
    fn record_install(&self, page_id: PageId, frame: &Frame);

    /// Called immediately before a frame is evicted.
    fn record_eviction(&self, frame_id: FrameId, frame: &Frame);

    /// Select a victim frame, or return `None` if no frame can be evicted.
    ///
    /// `frames` is the full frame array.  The implementation may attempt to lock
    /// individual frames; it must skip frames it cannot lock or that are
    /// pinned.
    fn select_victim(&self, frames: &[parking_lot::Mutex<Frame>]) -> Option<FrameId>;

    /// Human-readable policy name, used in diagnostics and tests.
    fn name(&self) -> &'static str;
}

/// Classic CLOCK policy: a single hand sweeps frames, clearing reference bits
/// and evicting the first unpinned frame with a clear reference bit.
#[derive(Debug)]
#[allow(dead_code)]
pub struct SimpleClock {
    hand: AtomicUsize,
}

impl SimpleClock {
    /// Create a CLOCK policy for a pool of the given capacity.
    pub fn new() -> Self {
        Self {
            hand: AtomicUsize::new(0),
        }
    }
}

impl Default for SimpleClock {
    fn default() -> Self {
        Self::new()
    }
}

impl EvictionPolicy for SimpleClock {
    fn record_access(&self, _frame_id: FrameId, frame: &Frame) {
        frame.ref_bit.store(true, Ordering::Relaxed);
    }

    fn record_install(&self, _page_id: PageId, frame: &Frame) {
        frame.ref_bit.store(true, Ordering::Relaxed);
    }

    fn record_eviction(&self, _frame_id: FrameId, _frame: &Frame) {}

    fn select_victim(&self, frames: &[parking_lot::Mutex<Frame>]) -> Option<FrameId> {
        let capacity = frames.len();
        for _ in 0..capacity * 4 {
            let hand = self.hand.fetch_add(1, Ordering::Relaxed) % capacity;
            let frame = frames[hand].try_lock()?;
            if frame.pin_count.load(Ordering::Acquire) > 0 {
                continue;
            }
            if frame.ref_bit.load(Ordering::Relaxed) {
                frame.ref_bit.store(false, Ordering::Relaxed);
                continue;
            }
            return Some(hand);
        }
        None
    }

    fn name(&self) -> &'static str {
        "simple-clock"
    }
}

/// CLOCK-Pro eviction policy.
///
/// Maintains three clock hands over the resident frame array plus a bounded
/// list of recently evicted cold pages (ghosts).  Pages with short reuse
/// distance are promoted to hot; pages with long reuse distance stay cold or
/// are evicted.  The relative size of the hot set adapts online based on hits
/// on ghost entries.
#[derive(Debug)]
pub struct ClockPro {
    capacity: usize,
    cold_hand: AtomicUsize,
    hot_hand: AtomicUsize,
    hot_target: AtomicUsize,
    /// Recently evicted cold pages, kept as metadata only.  The front of the
    /// deque is the oldest ghost, the back is the most recent.
    ghosts: Mutex<VecDeque<PageId>>,
    /// Maximum number of ghost entries.
    max_ghosts: usize,
}

impl ClockPro {
    /// Create a CLOCK-Pro policy for a pool of the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            cold_hand: AtomicUsize::new(0),
            hot_hand: AtomicUsize::new(0),
            hot_target: AtomicUsize::new((capacity / 2).max(1)),
            ghosts: Mutex::new(VecDeque::new()),
            max_ghosts: (capacity / MAX_GHOST_RATIO).max(1),
        }
    }

    /// True if `page_id` is in the ghost list (recently evicted cold page).
    pub fn is_ghost(&self, page_id: PageId) -> bool {
        self.ghosts.lock().contains(&page_id)
    }

    /// Remove a ghost entry if present.  Called when a ghost page is accessed
    /// again and should be promoted.
    pub fn remove_ghost(&self, page_id: PageId) {
        let mut ghosts = self.ghosts.lock();
        if let Some(pos) = ghosts.iter().position(|&x| x == page_id) {
            ghosts.remove(pos);
        }
    }

    /// Demote a hot page to cold-test.
    fn demote_hot(&self, frame: &mut Frame) {
        frame.set_hot(false);
        frame.set_cold_test(true);
    }

    /// Add a page id to the ghost list, evicting the oldest ghost if needed.
    fn add_ghost(&self, page_id: PageId) {
        let mut ghosts = self.ghosts.lock();
        if ghosts.len() >= self.max_ghosts && !ghosts.is_empty() {
            ghosts.pop_front();
        }
        ghosts.push_back(page_id);
    }

    /// Decrease the hot target when a cold page fails to earn promotion.
    fn decrease_hot_target(&self) {
        let current = self.hot_target.load(Ordering::Relaxed);
        let next = current.saturating_sub(1).max(1);
        self.hot_target.store(next, Ordering::Relaxed);
    }

    /// Number of frames currently classified as hot.
    fn hot_count(&self, frames: &[parking_lot::Mutex<Frame>]) -> usize {
        frames
            .iter()
            .filter(|f| {
                if let Some(frame) = f.try_lock() {
                    frame.is_hot()
                } else {
                    false
                }
            })
            .count()
    }
}

impl EvictionPolicy for ClockPro {
    fn record_access(&self, _frame_id: FrameId, frame: &Frame) {
        frame.ref_bit.store(true, Ordering::Relaxed);
        frame.bump_usage();

        if frame.is_hot() {
            return;
        }

        // A cold page that is referenced during its test period is promoted to
        // hot because it has demonstrated short reuse distance.
        if frame.is_cold_test() {
            frame.set_hot(true);
            frame.set_cold_test(false);
            let current = self.hot_target.load(Ordering::Relaxed);
            let next = (current + 1).min(self.capacity.saturating_sub(self.max_ghosts).max(1));
            self.hot_target.store(next, Ordering::Relaxed);
            return;
        }

        // A resident cold page that gets another reference moves to test.
        frame.set_cold_test(true);
    }

    fn record_install(&self, page_id: PageId, frame: &Frame) {
        frame.ref_bit.store(true, Ordering::Relaxed);
        if self.is_ghost(page_id) {
            // A recently evicted cold page was accessed again: promote directly
            // to hot and grow the hot target.
            self.remove_ghost(page_id);
            frame.set_hot(true);
            frame.set_cold_test(false);
            let current = self.hot_target.load(Ordering::Relaxed);
            let next = (current + 1).min(self.capacity.saturating_sub(self.max_ghosts).max(1));
            self.hot_target.store(next, Ordering::Relaxed);
        } else {
            frame.set_hot(false);
            frame.set_cold_test(true);
        }
    }

    fn record_eviction(&self, _frame_id: FrameId, frame: &Frame) {
        frame.set_hot(false);
        frame.set_cold_test(false);
    }

    fn select_victim(&self, frames: &[parking_lot::Mutex<Frame>]) -> Option<FrameId> {
        let capacity = frames.len();
        if capacity == 0 {
            return None;
        }

        // Run the hot hand first if the hot set exceeds its target.  This
        // demotes unreferenced hot pages, making them eligible as cold victims.
        let hot_target = self.hot_target.load(Ordering::Relaxed);
        if self.hot_count(frames) > hot_target {
            for _ in 0..capacity * 2 {
                let hand = self.hot_hand.fetch_add(1, Ordering::Relaxed) % capacity;
                let mut frame = match frames[hand].try_lock() {
                    Some(g) => g,
                    None => continue,
                };
                if frame.pin_count.load(Ordering::Acquire) > 0 {
                    continue;
                }
                if !frame.is_hot() {
                    continue;
                }
                if frame.ref_bit.load(Ordering::Relaxed) {
                    frame.ref_bit.store(false, Ordering::Relaxed);
                    continue;
                }
                self.demote_hot(&mut frame);
                break;
            }
        }

        // Run the cold hand to find a resident victim.
        for _ in 0..capacity * 4 {
            let hand = self.cold_hand.fetch_add(1, Ordering::Relaxed) % capacity;
            let frame = match frames[hand].try_lock() {
                Some(g) => g,
                None => continue,
            };
            if frame.pin_count.load(Ordering::Acquire) > 0 {
                continue;
            }

            // Free frame: use immediately.
            if frame.page_id == NULL_PAGE_ID {
                return Some(hand);
            }

            // Hot pages are protected by the hot hand; the cold hand must not
            // touch their reference bit, otherwise a subsequent hot-hand sweep
            // would incorrectly demote a page that was merely inspected by the
            // cold hand.  Skip them unconditionally.
            if frame.is_hot() {
                continue;
            }

            // Cold (or cold-test) page.
            if frame.ref_bit.load(Ordering::Relaxed) {
                frame.ref_bit.store(false, Ordering::Relaxed);
                // If this was a brand-new cold page, it now survives its first
                // test period.
                frame.set_cold_test(false);
                continue;
            }

            // Evict this cold page.
            if frame.is_cold_test() {
                // It was evicted during its test period: keep metadata so a
                // future hit can promote it to hot.
                self.add_ghost(frame.page_id);
            } else {
                // It survived its test period without being referenced again.
                self.decrease_hot_target();
            }
            return Some(hand);
        }

        // Final fallback: simple CLOCK sweep.  Should only be reached if the
        // pool is pathologically full of pinned pages.
        for _ in 0..capacity * 2 {
            let hand = self.cold_hand.fetch_add(1, Ordering::Relaxed) % capacity;
            let frame = match frames[hand].try_lock() {
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
            return Some(hand);
        }

        None
    }

    fn name(&self) -> &'static str {
        "clock-pro"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_clock_selects_unreferenced_frame() {
        // This test is exercised more thoroughly through the buffer-pool
        // integration tests; here we just sanity-check the trait wiring.
        let policy = SimpleClock::new();
        assert_eq!(policy.name(), "simple-clock");
    }

    #[test]
    fn clock_pro_tracks_ghosts() {
        let policy = ClockPro::new(8);
        assert!(!policy.is_ghost(PageId::new(1)));
        policy.add_ghost(PageId::new(1));
        assert!(policy.is_ghost(PageId::new(1)));
        policy.remove_ghost(PageId::new(1));
        assert!(!policy.is_ghost(PageId::new(1)));
    }

    #[test]
    fn clock_pro_ghost_cap_is_enforced() {
        let policy = ClockPro::new(4);
        assert_eq!(policy.max_ghosts, 2);
        policy.add_ghost(PageId::new(1));
        policy.add_ghost(PageId::new(2));
        policy.add_ghost(PageId::new(3));
        let ghosts = policy.ghosts.lock();
        assert_eq!(ghosts.len(), 2);
        assert!(!ghosts.contains(&PageId::new(1)));
    }
}
