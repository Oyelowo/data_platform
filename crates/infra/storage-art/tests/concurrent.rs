//! Concurrent integration tests for `storage-art`.

use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use storage_art::{ArtMap, ArtMapOptions};

#[test]
fn concurrent_inserts() {
    let map = Arc::new(ArtMap::new(ArtMapOptions::default()));
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("t{}-i{}", t, i);
                    let value = format!("v{}", i);
                    map.insert(key.as_bytes(), value.as_bytes()).unwrap();
                }
            })
        })
        .collect();

    for handle in threads {
        handle.join().unwrap();
    }

    for t in 0..8 {
        for i in 0..100 {
            let key = format!("t{}-i{}", t, i);
            let value = format!("v{}", i);
            assert_eq!(
                map.get(key.as_bytes()),
                Some(Bytes::from(value)),
                "missing {}",
                key
            );
        }
    }
}

#[test]
fn concurrent_read_write() {
    let map = Arc::new(ArtMap::new(ArtMapOptions::default()));

    let writer = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for i in 0..500 {
                let key = format!("key{:03}", i);
                map.insert(key.as_bytes(), b"written").unwrap();
            }
        })
    };

    let reader = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for _ in 0..1_000 {
                let _ = map.get(b"key000");
            }
        })
    };

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn concurrent_scans() {
    let map = Arc::new(ArtMap::new(ArtMapOptions::default()));
    for i in 0..100 {
        let key = format!("key{:03}", i);
        map.insert(key.as_bytes(), b"v").unwrap();
    }

    let scanner = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for _ in 0..100 {
                let cursor = map.range(Some(b"key"), Some(b"kez"));
                let mut last: Option<Vec<u8>> = None;
                let mut count = 0;
                for item in cursor {
                    let (k, _) = item.unwrap();
                    if let Some(ref last) = last {
                        assert!(k.as_ref() >= last.as_slice(), "scan not sorted");
                    }
                    last = Some(k.to_vec());
                    count += 1;
                }
                assert!(count > 0, "scan should not be empty");
            }
        })
    };

    let writer = {
        let map = Arc::clone(&map);
        thread::spawn(move || {
            for i in 100..200 {
                let key = format!("key{:03}", i);
                map.insert(key.as_bytes(), b"v").unwrap();
            }
        })
    };

    scanner.join().unwrap();
    writer.join().unwrap();
}
