//! Long-running stress tests for the lock-free skip-map.
//!
//! These tests are intentionally not run by default because they can take a
//! long time. Run them explicitly with:
//!
//! ```bash
//! cargo test -p storage-skipmap --test stress -- --nocapture --ignored
//! ```

use std::sync::Arc;
use std::thread;

use storage_skipmap::SkipMap;

const KEYS: usize = 1000;
const ROUNDS: usize = 10_000;

/// Mixed concurrent workload: insert, remove, get, and iterate.
#[test]
#[ignore]
fn mixed_workload_no_crash() {
    let map = Arc::new(SkipMap::<usize, usize>::new());
    let mut handles = Vec::new();

    for t in 0..4 {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for round in 0..ROUNDS {
                let key = (round * 7 + t * 13) % KEYS;
                match round % 4 {
                    0 => {
                        map.insert(key, round);
                    }
                    1 => {
                        map.remove(&key);
                    }
                    2 => {
                        let _ = map.get(&key);
                    }
                    3 => {
                        let _ = map.iter();
                    }
                    _ => unreachable!(),
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // After the workload, the map must still be queryable and ordered.
    let snapshot: Vec<_> = map.iter();
    for window in snapshot.windows(2) {
        assert!(window[0].0 < window[1].0, "scan not sorted");
    }
}

/// Many threads repeatedly overwrite the same small set of keys.
#[test]
#[ignore]
fn contention_overwrite() {
    let map = Arc::new(SkipMap::<usize, usize>::new());
    let mut handles = Vec::new();

    for t in 0..8 {
        let map = Arc::clone(&map);
        handles.push(thread::spawn(move || {
            for round in 0..ROUNDS {
                let key = round % 16;
                map.insert(key, t * ROUNDS + round);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Every key 0..16 must be present with some value.
    let snapshot: std::collections::BTreeMap<usize, usize> = map.iter().into_iter().collect();
    for k in 0..16 {
        assert!(snapshot.contains_key(&k), "missing key {}", k);
    }
}
