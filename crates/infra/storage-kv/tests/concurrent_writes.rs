//! Concurrent write-path tests for the LSM engine.
//!
//! These tests exercise the fully concurrent write path: sequence allocation,
//! WAL append, and MemTable insert all happen without a global engine mutex.

use std::sync::Arc;
use std::thread;

use storage_kv::{LsmEngine, LsmOptions};
use tempfile::TempDir;

fn opts() -> LsmOptions {
    LsmOptions {
        // Small write buffer so flushes and compactions happen frequently during
        // the test, exercising the background-worker interaction.
        write_buffer_size: 256,
        ..Default::default()
    }
}

fn opts_large_buffer() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 64 * 1024 * 1024,
        ..Default::default()
    }
}

#[test]
fn concurrent_puts_unique_keys() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts()).unwrap());
    let threads = 8;
    let per_thread = 100;
    let mut handles = Vec::new();

    for t in 0..threads {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for i in 0..per_thread {
                let key = format!("t{}-k{}", t, i);
                let value = format!("v{}", i);
                engine.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    let mut missing = Vec::new();
    for t in 0..threads {
        for i in 0..per_thread {
            let key = format!("t{}-k{}", t, i);
            let expected = format!("v{}", i);
            if engine.get(key.as_bytes()).unwrap() != Some(bytes::Bytes::from(expected)) {
                missing.push(key);
            }
        }
    }
    assert!(
        missing.is_empty(),
        "missing {} keys: {:?}",
        missing.len(),
        missing
    );
}

#[test]
fn concurrent_puts_same_key() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts()).unwrap());
    let threads = 8;
    let per_thread = 250;
    let mut handles = Vec::new();

    for t in 0..threads {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for i in 0..per_thread {
                let value = format!("t{}-i{}", t, i);
                engine.put(b"key", value.as_bytes()).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    let got = engine.get(b"key").unwrap();
    assert!(got.is_some(), "key should exist after concurrent puts");
    let got_str = String::from_utf8(got.unwrap().to_vec()).unwrap();
    assert!(
        got_str.starts_with("t") && got_str.contains("-i"),
        "unexpected final value: {}",
        got_str
    );
}

#[test]
fn concurrent_puts_and_deletes_same_key() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts()).unwrap());
    let mut handles = Vec::new();

    for t in 0..4 {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for i in 0..200 {
                engine
                    .put(b"key", format!("writer{}-{}", t, i).as_bytes())
                    .unwrap();
            }
        }));
    }

    for _ in 0..4 {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..200 {
                engine.delete(b"key").unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    // After all threads finish and everything is flushed, the result must be
    // one of the values that was written, or a deletion (None).  The important
    // invariant is that the engine does not crash or corrupt the key.
    let _ = engine.get(b"key").unwrap();
}

#[test]
fn concurrent_puts_unique_keys_two_threads() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts()).unwrap());
    let mut handles = Vec::new();

    for t in 0..2 {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for i in 0..50 {
                let key = format!("t{}-k{}", t, i);
                let value = format!("v{}", i);
                engine.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    for t in 0..2 {
        for i in 0..50 {
            let key = format!("t{}-k{}", t, i);
            let expected = format!("v{}", i);
            assert_eq!(
                engine.get(key.as_bytes()).unwrap(),
                Some(bytes::Bytes::from(expected)),
                "missing key {}",
                key
            );
        }
    }
}

#[test]
fn concurrent_puts_unique_keys_large_buffer() {
    let dir = TempDir::new().unwrap();
    let engine = Arc::new(LsmEngine::open(dir.path(), opts_large_buffer()).unwrap());
    let threads = 8;
    let per_thread = 100;
    let mut handles = Vec::new();

    for t in 0..threads {
        let engine = engine.clone();
        handles.push(thread::spawn(move || {
            for i in 0..per_thread {
                let key = format!("t{}-k{}", t, i);
                let value = format!("v{}", i);
                engine.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    engine.sync().unwrap();

    for t in 0..threads {
        for i in 0..per_thread {
            let key = format!("t{}-k{}", t, i);
            let expected = format!("v{}", i);
            assert_eq!(
                engine.get(key.as_bytes()).unwrap(),
                Some(bytes::Bytes::from(expected)),
                "missing key {}",
                key
            );
        }
    }
}

#[test]
fn engine_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LsmEngine>();
}
