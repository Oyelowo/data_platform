//! Miri-compatible smoke tests.
//!
//! Run with:
//! ```bash
//! MIRIFLAGS="-Zmiri-tree-borrows -Zmiri-ignore-leaks" \
//!   cargo +nightly miri test -p storage-skipmap --test miri_smoke
//! ```
//!
//! `-Zmiri-ignore-leaks` is required because `crossbeam-epoch` defers node
//! reclamation and keeps thread-local state that Miri cannot prove is freed
//! before program exit.

use storage_skipmap::SkipMap;

#[test]
fn miri_basic_crud() {
    let map = SkipMap::new();
    map.insert(1, 10);
    map.insert(2, 20);
    assert_eq!(map.get(&1), Some(10));
    assert_eq!(map.get(&2), Some(20));
    assert_eq!(map.insert(1, 11), Some(10));
    assert_eq!(map.get(&1), Some(11));
    assert_eq!(map.remove(&1), Some(11));
    assert_eq!(map.get(&1), None);
}

#[test]
fn miri_iteration() {
    let map = SkipMap::new();
    map.insert(3, 30);
    map.insert(1, 10);
    map.insert(2, 20);
    let entries: Vec<_> = map.iter().collect();
    assert_eq!(entries, vec![(1, 10), (2, 20), (3, 30)]);
}

#[test]
fn miri_range() {
    let map = SkipMap::new();
    map.insert(1, 10);
    map.insert(2, 20);
    map.insert(3, 30);
    assert_eq!(
        map.range(Some(&1), Some(&3)).collect::<Vec<_>>(),
        vec![(1, 10), (2, 20)]
    );
}
