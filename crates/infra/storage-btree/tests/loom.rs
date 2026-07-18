//! Loom model tests for the Optimistic Lock Coupling (OLC) protocol.
//!
//! These tests are deliberately abstract: they model the latch/ordering
//! invariants the real `v2` tree relies on, rather than the full buffer pool
//! and on-disk format.  This keeps the state space small enough for Loom to
//! exhaust while still covering the concurrency hazards we care about:
//!
//! 1. A reader must observe a consistent root-to-leaf snapshot.
//! 2. A root split must be detected and retried.
//! 3. Structure-modifying operations must acquire latches in a fixed order
//!    (top-down, low-page-id first) to avoid deadlock and torn structural
//!    updates.

use loom::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

/// A latch-free "page" for the model: an atomic value plus an atomic version
/// word.  The version is even when unlocked and odd when a writer is in
/// progress, exactly like the real page OLC latch word.
struct ModelPage {
    value: AtomicU64,
    version: AtomicU64,
}

impl ModelPage {
    fn new(value: u64) -> Self {
        Self {
            value: AtomicU64::new(value),
            version: AtomicU64::new(0),
        }
    }
}

/// Optimistically read `page`, returning `Some(value)` iff the version was
/// stable and even for the whole read.
fn optimistic_read(page: &ModelPage) -> Option<u64> {
    let before = page.version.load(Ordering::Acquire);
    if before & 1 != 0 {
        return None;
    }
    let value = page.value.load(Ordering::Acquire);
    let after = page.version.load(Ordering::Acquire);
    if before == after { Some(value) } else { None }
}

/// Write `new_value` to `page` while toggling the version word, so any
/// concurrent optimistic reader sees either the old or the new value.
fn write_versioned(page: &ModelPage, new_value: u64) {
    page.version.fetch_add(1, Ordering::AcqRel);
    page.value.store(new_value, Ordering::Release);
    page.version.fetch_add(1, Ordering::Release);
}

/// Model 1: a root-to-leaf optimistic read never observes a torn value, even
/// when a writer modifies the leaf concurrently.
///
/// Reader: perform a single optimistic read.  Writer: perform a single
/// versioned write.  If the reader happens to observe a stable snapshot, the
/// value must be one the writer actually stored.
#[test]
fn optimistic_root_to_leaf_read_is_consistent() {
    loom::model(|| {
        let leaf = Arc::new(ModelPage::new(0));

        let reader_leaf = Arc::clone(&leaf);
        let reader = thread::spawn(move || optimistic_read(&reader_leaf));

        let writer = thread::spawn(move || {
            write_versioned(&leaf, 1);
        });

        let result = reader.join().unwrap();
        writer.join().unwrap();

        if let Some(v) = result {
            assert!(v == 0 || v == 1);
        }
    });
}

/// Model 2: a root split is detected by re-checking the root pointer after the
/// optimistic leaf read.
///
/// Layout: root id points to a leaf.  The writer "splits" the root by moving
/// the canonical root pointer to a new page; the reader must either observe the
/// old value *before* the split or retry and observe the new value, never a
/// stale value from the post-split perspective.
#[test]
fn root_split_race_is_detected() {
    loom::model(|| {
        let root = Arc::new(AtomicUsize::new(0));
        let old_leaf = Arc::new(ModelPage::new(10));
        let new_leaf = Arc::new(ModelPage::new(20));

        let pages: Vec<Arc<ModelPage>> = vec![Arc::clone(&old_leaf), Arc::clone(&new_leaf)];

        let reader_root = Arc::clone(&root);
        let reader_pages: Vec<Arc<ModelPage>> = pages.iter().map(Arc::clone).collect();

        let reader = thread::spawn(move || {
            let root_id_before = reader_root.load(Ordering::Acquire);
            let page = &reader_pages[root_id_before];
            let value = optimistic_read(page);
            let root_id_after = reader_root.load(Ordering::Acquire);
            (root_id_before, root_id_after, value)
        });

        let writer = thread::spawn(move || {
            // Lock the old leaf (odd version), publish the new root, unlock.
            old_leaf.version.fetch_add(1, Ordering::AcqRel);
            root.store(1, Ordering::Release);
            old_leaf.version.fetch_add(1, Ordering::Release);
        });

        let (before, after, value) = reader.join().unwrap();
        writer.join().unwrap();

        // If the reader observed a stable root id and a stable page version,
        // the value must belong to the page it believes it read.
        if before == after
            && let Some(v) = value
        {
            let expected = if after == 0 { 10 } else { 20 };
            assert_eq!(v, expected);
        }
    });
}

/// Model 3: structure-modifying operations acquire latches in a fixed order
/// (parent before child) and therefore cannot deadlock.
///
/// Two writers each want to update a child under the same parent.  They use
/// blocking `Mutex` latches acquired top-down.  Loom verifies that no
/// interleaving deadlocks and that the shared counter ends at the expected
/// value.
#[test]
fn smo_top_down_latch_order_is_deadlock_free() {
    loom::model(|| {
        let parent = Arc::new(Mutex::new(()));
        let child_a = Arc::new(Mutex::new(()));
        let child_b = Arc::new(Mutex::new(()));

        let shared = Arc::new(AtomicU64::new(0));

        let mut handles = Vec::new();
        for _ in 0..2 {
            let parent = Arc::clone(&parent);
            let child_a = Arc::clone(&child_a);
            let child_b = Arc::clone(&child_b);
            let shared = Arc::clone(&shared);
            handles.push(thread::spawn(move || {
                // Acquire parent, then children in page-id (i.e. left-to-right)
                // order.  This matches the real tree's SMO latch ordering.
                let _p = parent.lock().unwrap();
                let _a = child_a.lock().unwrap();
                let _b = child_b.lock().unwrap();
                let before = shared.load(Ordering::Acquire);
                shared.store(before + 3, Ordering::Release);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(shared.load(Ordering::Acquire), 6);
    });
}
