//! Concurrency and isolation conformance tests.

use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use storage_traits::{Engine, Transaction, TxnOptions};

/// Run all concurrency conformance tests against `factory`.
pub fn run<E, F>(factory: &F)
where
    E: Engine,
    F: Fn() -> E,
{
    concurrent_puts(factory);
    concurrent_read_write(factory);
    concurrent_scans(factory);
}

fn concurrent_puts<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = Arc::new(factory());
    let threads: Vec<_> = (0..8)
        .map(|t| {
            let engine = Arc::clone(&engine);
            thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("t{}-i{}", t, i);
                    let value = format!("v{}", i);
                    let mut tx = engine.begin(TxnOptions::default()).unwrap();
                    tx.put(key.as_bytes(), value.as_bytes()).unwrap();
                    tx.commit().unwrap();
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
                engine.get(key.as_bytes()).unwrap(),
                Some(Bytes::from(value))
            );
        }
    }
}

fn concurrent_read_write<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = Arc::new(factory());

    let writer = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for i in 0..500 {
                let key = format!("key{:03}", i);
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                tx.put(key.as_bytes(), b"written").unwrap();
                tx.commit().unwrap();
            }
        })
    };

    let reader = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for _ in 0..1_000 {
                let _ = engine.get(b"key000");
            }
        })
    };

    writer.join().unwrap();
    reader.join().unwrap();
}

fn concurrent_scans<E: Engine, F: Fn() -> E>(factory: &F) {
    let engine = Arc::new(factory());

    let mut tx = engine.begin(TxnOptions::default()).unwrap();
    for i in 0..100 {
        let key = format!("key{:03}", i);
        tx.put(key.as_bytes(), b"v").unwrap();
    }
    tx.commit().unwrap();

    let scanner = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for _ in 0..100 {
                let cursor = engine.scan(Some(b"key"), Some(b"kez")).unwrap();
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
                // Under read-committed isolation the count may grow as the
                // writer commits; we only require the scan to be sorted.
                assert!(count > 0, "scan should not be empty");
            }
        })
    };

    let writer = {
        let engine = Arc::clone(&engine);
        thread::spawn(move || {
            for i in 100..200 {
                let key = format!("key{:03}", i);
                let mut tx = engine.begin(TxnOptions::default()).unwrap();
                tx.put(key.as_bytes(), b"v").unwrap();
                tx.commit().unwrap();
            }
        })
    };

    scanner.join().unwrap();
    writer.join().unwrap();
}
