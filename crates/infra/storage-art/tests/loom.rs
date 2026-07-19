//! Loom model-check tests for the `storage-art` Optimistic Lock Coupling paths.

#![cfg(loom)]

use std::sync::Arc;

use bytes::Bytes;
use loom::thread;
use storage_art::{ArtMap, ArtMapOptions};

#[test]
#[cfg(loom)]
fn loom_concurrent_insert_get() {
    loom::model(|| {
        let map = Arc::new(ArtMap::new(ArtMapOptions::default()));

        let t1 = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                map.insert(b"a", b"1").unwrap();
            })
        };
        let t2 = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                map.insert(b"b", b"2").unwrap();
            })
        };

        t1.join().unwrap();
        t2.join().unwrap();

        let a = map.get(b"a");
        let b = map.get(b"b");
        assert!(a == Some(Bytes::from_static(b"1")) || a.is_none());
        assert!(b == Some(Bytes::from_static(b"2")) || b.is_none());
        // At least one writer must be visible after both join.
        assert!(a.is_some() || b.is_some());
    });
}
