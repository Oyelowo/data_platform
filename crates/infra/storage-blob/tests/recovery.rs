//! Recovery tests for `storage-blob`.

use std::io::Read;

use storage_blob::{BlobStoreImpl, BlobStoreOptions};
use storage_traits::BlobStore;
use tempfile::TempDir;

fn open(dir: &TempDir) -> BlobStoreImpl {
    BlobStoreImpl::open(dir.path(), BlobStoreOptions::default()).unwrap()
}

#[test]
fn data_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);
    let payload = b"durable payload";
    store.put(b"key", &mut &payload[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    let store = open(&dir);
    let mut reader = store.get(b"key").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, payload);
}

#[test]
fn delete_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);
    store.put(b"key", &mut &b"data"[..]).unwrap();
    store.delete(b"key").unwrap();
    store.sync().unwrap();
    drop(store);

    let store = open(&dir);
    assert!(store.get(b"key").is_err());
    assert_eq!(store.size(b"key").unwrap(), None);
}

#[test]
fn many_objects_survive_reopen() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);
    for i in 0..50u8 {
        let id = vec![b'k', i];
        let payload = vec![i; 4096];
        store.put(&id, &mut &payload[..]).unwrap();
    }
    store.sync().unwrap();
    drop(store);

    let store = open(&dir);
    for i in 0..50u8 {
        let id = vec![b'k', i];
        let mut reader = store.get(&id).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, vec![i; 4096]);
    }
}
