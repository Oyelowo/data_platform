//! Basic round-trip tests for `storage-blob`.

use std::io::Read;

use storage_blob::{BlobStoreImpl, BlobStoreOptions};
use storage_traits::BlobStore;
use tempfile::TempDir;

fn open(options: BlobStoreOptions) -> (TempDir, BlobStoreImpl) {
    let dir = TempDir::new().unwrap();
    let store = BlobStoreImpl::open(dir.path(), options).unwrap();
    (dir, store)
}

#[test]
fn put_get_small_object() {
    let (_dir, store) = open(BlobStoreOptions::default());
    let id = b"small";
    let bytes = b"hello world";
    let written = store.put(id, &mut &bytes[..]).unwrap();
    assert_eq!(written, bytes.len() as u64);

    let mut reader = store.get(id).unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, bytes);
    assert_eq!(store.size(id).unwrap(), Some(bytes.len() as u64));
}

#[test]
fn put_get_large_object() {
    let (_dir, store) = open(BlobStoreOptions::default());
    let id = b"large";
    let payload = vec![0xABu8; 4 * 1024 * 1024];
    let written = store.put(id, &mut &payload[..]).unwrap();
    assert_eq!(written, payload.len() as u64);

    let mut reader = store.get(id).unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, payload);
}

#[test]
fn empty_object_roundtrips() {
    let (_dir, store) = open(BlobStoreOptions::default());
    let id = b"empty";
    let mut empty = &[][..];
    store.put(id, &mut empty).unwrap();

    let mut reader = store.get(id).unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert!(buf.is_empty());
    assert_eq!(store.size(id).unwrap(), Some(0));
}

#[test]
fn delete_removes_object() {
    let (_dir, store) = open(BlobStoreOptions::default());
    let id = b"gone";
    store.put(id, &mut &b"data"[..]).unwrap();
    store.delete(id).unwrap();
    assert!(store.get(id).is_err());
    assert_eq!(store.size(id).unwrap(), None);
}

#[test]
fn delete_missing_is_ok() {
    let (_dir, store) = open(BlobStoreOptions::default());
    store.delete(b"never-existed").unwrap();
}

#[test]
fn sync_does_not_error() {
    let (_dir, store) = open(BlobStoreOptions::default());
    store.put(b"x", &mut &b"y"[..]).unwrap();
    store.sync().unwrap();
}

#[test]
fn multiple_objects_isolated() {
    let (_dir, store) = open(BlobStoreOptions::default());
    for i in 0..100u8 {
        let id = vec![b'k', i];
        let payload = vec![i; 1024];
        store.put(&id, &mut &payload[..]).unwrap();
    }
    for i in 0..100u8 {
        let id = vec![b'k', i];
        let mut reader = store.get(&id).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, vec![i; 1024]);
    }
}
