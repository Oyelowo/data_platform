//! Concurrency tests for `storage-blob`.

use std::io::Read;
use std::sync::Arc;
use std::thread;

use storage_blob::BlobStoreImpl;
use storage_traits::BlobStore;
use tempfile::TempDir;

#[test]
fn concurrent_disjoint_writes() {
    let dir = TempDir::new().unwrap();
    let store = BlobStoreImpl::open(dir.path(), Default::default()).unwrap();
    let store = std::sync::Arc::new(store);

    let mut handles = Vec::new();
    for t in 0..8u8 {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            for i in 0..50u8 {
                let id = vec![b't', t, i];
                let payload = vec![t ^ i; 1024];
                store.put(&id, &mut &payload[..]).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    for t in 0..8u8 {
        for i in 0..50u8 {
            let id = vec![b't', t, i];
            let mut reader = store.get(&id).unwrap();
            let mut buf = Vec::new();
            reader.read_to_end(&mut buf).unwrap();
            assert_eq!(buf, vec![t ^ i; 1024], "mismatch for t={} i={}", t, i);
        }
    }
}

#[test]
fn readers_do_not_block_writer() {
    let dir = TempDir::new().unwrap();
    let store = BlobStoreImpl::open(dir.path(), Default::default()).unwrap();
    let store = std::sync::Arc::new(store);

    // Pre-populate.
    let big = vec![0x55u8; 2 * 1024 * 1024];
    store.put(b"big", &mut &big[..]).unwrap();

    let reader_store = Arc::clone(&store);
    let reader = thread::spawn(move || {
        let mut reader = reader_store.get(b"big").unwrap();
        let mut buf = vec![0u8; 1024];
        // Slow, partial read.
        reader.read_exact(&mut buf).unwrap();
        buf
    });

    // Writer should proceed without waiting for the reader to finish.
    store.put(b"small", &mut &b"x"[..]).unwrap();

    let _ = reader.join().unwrap();
    let mut r = store.get(b"small").unwrap();
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"x");
}
