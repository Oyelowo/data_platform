//! Garbage-collection tests for `storage-blob`.

use std::io::Read;

use storage_blob::{BlobStoreImpl, BlobStoreOptions};
use storage_traits::BlobStore;
use tempfile::TempDir;

fn open_with_small_volumes(dir: &TempDir) -> BlobStoreImpl {
    let opts = BlobStoreOptions {
        max_volume_size: 64 * 1024, // 64 KiB volumes for easy fragmentation
        gc_dead_ratio_threshold: 0.1,
        ..Default::default()
    };
    BlobStoreImpl::open(dir.path(), opts).unwrap()
}

#[test]
fn gc_reclaims_deleted_objects() {
    let dir = TempDir::new().unwrap();
    let store = open_with_small_volumes(&dir);

    // Write enough objects to fill several volumes.
    let mut ids = Vec::new();
    for i in 0..20u8 {
        let id = vec![b'k', i];
        let payload = vec![i; 4096];
        store.put(&id, &mut &payload[..]).unwrap();
        ids.push(id);
    }

    // Delete half of them.
    for id in &ids[..10] {
        store.delete(id).unwrap();
    }

    store.force_gc().unwrap();

    // Live objects still readable.
    for id in &ids[10..] {
        let mut reader = store.get(id).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(buf.len(), 4096);
    }

    // Deleted objects stay gone.
    for id in &ids[..10] {
        assert!(store.get(id).is_err());
    }
}

#[test]
fn gc_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let store = open_with_small_volumes(&dir);

    store.put(b"keep", &mut &vec![1u8; 4096][..]).unwrap();
    store.put(b"delete", &mut &vec![2u8; 4096][..]).unwrap();
    store.delete(b"delete").unwrap();
    store.force_gc().unwrap();
    store.sync().unwrap();
    drop(store);

    let store = open_with_small_volumes(&dir);
    let mut reader = store.get(b"keep").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, vec![1u8; 4096]);
    assert!(store.get(b"delete").is_err());
}

#[test]
fn readers_survive_gc_volume_deletion() {
    let dir = TempDir::new().unwrap();
    let store = open_with_small_volumes(&dir);

    // Place two objects in the first volume.
    store.put(b"keep", &mut &vec![1u8; 1024][..]).unwrap();
    store.put(b"delete", &mut &vec![2u8; 50 * 1024][..]).unwrap();

    // Rotate to a second volume so the first one is eligible for GC.
    store.put(b"rotate", &mut &vec![3u8; 20 * 1024][..]).unwrap();

    // Open a reader before GC deletes the original volume file.
    let mut old_reader = store.get(b"keep").unwrap();
    let mut old_buf = Vec::new();
    old_reader.read_exact(&mut [0u8; 100]).unwrap();

    // Delete the large object and run GC; the old volume must be unlinked.
    store.delete(b"delete").unwrap();
    store.force_gc().unwrap();

    // The reader that holds the now-unlinked file descriptor still works.
    old_reader.read_to_end(&mut old_buf).unwrap();
    assert_eq!(old_buf.len(), 1024 - 100);
    assert!(old_buf.iter().all(|&b| b == 1));

    // New readers use the rewritten location.
    let mut new_reader = store.get(b"keep").unwrap();
    let mut new_buf = Vec::new();
    new_reader.read_to_end(&mut new_buf).unwrap();
    assert_eq!(new_buf, vec![1u8; 1024]);
}
