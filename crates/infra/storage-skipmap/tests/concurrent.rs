//! Concurrent correctness tests for the lock-free skip-map.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::thread;

use storage_skipmap::SkipMap;

/// Many threads insert disjoint key ranges. Every key must be present and the
/// iteration order must be sorted.
#[test]
fn concurrent_disjoint_inserts() {
    const THREADS: usize = 8;
    const PER_THREAD: usize = 1000;

    let map = Arc::new(SkipMap::<usize, usize>::new());
    let mut handles = Vec::new();

    for t in 0..THREADS {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for i in 0..PER_THREAD {
                let key = t * PER_THREAD + i;
                map.insert(key, key);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(map.len(), THREADS * PER_THREAD);

    let snapshot: Vec<_> = map.iter().collect();
    assert_eq!(snapshot.len(), THREADS * PER_THREAD);

    for (expected, (k, v)) in snapshot.iter().enumerate() {
        assert_eq!(*k, expected);
        assert_eq!(*v, expected);
    }
}

/// Two threads repeatedly insert and remove overlapping keys. The final state
/// must be sorted and contain no duplicates.
#[test]
fn concurrent_overlapping_mutate_invariants() {
    const ROUNDS: usize = 5000;

    let map = Arc::new(SkipMap::<usize, usize>::new());

    let inserter = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for round in 0..ROUNDS {
                map.insert(round % 64, round);
            }
        })
    };

    let remover = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for round in 0..ROUNDS {
                map.remove(&(round % 64));
            }
        })
    };

    inserter.join().unwrap();
    remover.join().unwrap();

    let snapshot: Vec<_> = map.iter().collect();
    let keys: BTreeSet<_> = snapshot.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys.len(),
        snapshot.len(),
        "duplicate keys in final snapshot"
    );

    for window in snapshot.windows(2) {
        assert!(window[0].0 < window[1].0, "snapshot not sorted");
    }
}

/// Concurrent replacements of the same key must leave the map with exactly one
/// entry for that key, and the visible value must be one of the inserted
/// values.
#[test]
fn concurrent_replace_same_key() {
    const THREADS: usize = 8;
    const ROUNDS: usize = 1000;

    let map = Arc::new(SkipMap::<usize, usize>::new());
    map.insert(0, 0);

    let mut handles = Vec::new();
    for t in 0..THREADS {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for round in 0..ROUNDS {
                map.insert(0, t * ROUNDS + round + 1);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let snapshot: Vec<_> = map.iter().collect();
    assert_eq!(snapshot.len(), 1);
    let value = snapshot[0].1;
    assert!(value <= THREADS * ROUNDS, "unexpected value {}", value);
}
