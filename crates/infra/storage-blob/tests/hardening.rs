//! Hardening-phase tests for the production-readiness fixes in `storage-blob`.
//!
//! These tests correspond to section 4 of
//! `.doc/notes/hardening-phase/DESIGN.md`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use storage_blob::{BlobStoreImpl, BlobStoreOptions};
use storage_traits::BlobStore;
use tempfile::TempDir;

fn open(options: BlobStoreOptions) -> (TempDir, BlobStoreImpl) {
    let dir = TempDir::new().unwrap();
    let store = BlobStoreImpl::open(dir.path(), options).unwrap();
    (dir, store)
}

fn active_volume_path(dir: &Path) -> PathBuf {
    let volumes_dir = dir.join("volumes");
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(&volumes_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".blob")
            && let Ok(n) = stem.parse::<u64>()
        {
            let path = entry.path();
            if best.as_ref().map(|(bn, _)| n > *bn).unwrap_or(true) {
                best = Some((n, path));
            }
        }
    }
    best.expect("no volume files found").1
}

#[test]
fn sync_on_put_true_preserves_data_across_reopen() {
    let opts = BlobStoreOptions {
        sync_on_put: true,
        ..Default::default()
    };
    let (dir, store) = open(opts.clone());
    store.put(b" durable ", &mut &b"payload"[..]).unwrap();
    // No explicit sync() here: put must have made the data durable.
    drop(store);

    let store = BlobStoreImpl::open(dir.path(), opts).unwrap();
    let mut reader = store.get(b" durable ").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"payload");
}

#[test]
fn corrupted_header_length_returns_error() {
    let (dir, store) = open(BlobStoreOptions::default());
    store.put(b"fragile", &mut &b"fragile payload"[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    let volume_path = active_volume_path(dir.path());
    let mut bytes = fs::read(&volume_path).unwrap();

    // Corrupt payload_len (bytes 12..20) to a huge value.
    bytes[12..20].copy_from_slice(&(1u64 << 40).to_le_bytes());
    // Recompute the header checksum so the header appears internally valid.
    let header_size = storage_blob::format::HEADER_SIZE;
    let crc = crc32c::crc32c(&bytes[..header_size - 4]);
    bytes[header_size - 4..header_size].copy_from_slice(&crc.to_le_bytes());
    fs::write(&volume_path, bytes).unwrap();

    let store = BlobStoreImpl::open(dir.path(), BlobStoreOptions::default()).unwrap();
    // Recovery may truncate the impossible record, or get may fail when it
    // detects the length mismatch.  Either is acceptable; the bug would be an
    // OOM or a successful read of garbage.
    let result = store.get(b"fragile");
    if let Ok(mut reader) = result {
        let err = reader.read_to_end(&mut Vec::new()).unwrap_err();
        assert!(
            err.to_string().contains("corrupt") || err.to_string().contains("crc"),
            "unexpected error: {err}"
        );
    }
}

#[test]
fn concurrent_overlapping_ops_recover_consistently() {
    let opts = BlobStoreOptions {
        max_volume_size: 64 * 1024,
        gc_dead_ratio_threshold: 0.1,
        background_gc: false,
        background_gc_interval: std::time::Duration::from_secs(1),
        ..Default::default()
    };
    let (dir, store) = open(opts.clone());
    let store = Arc::new(store);

    // A small set of IDs that threads will race over.
    let ids: Vec<Vec<u8>> = (0..8u8).map(|i| vec![b'k', i]).collect();

    let mut handles = Vec::new();
    for t in 0..4usize {
        let store = Arc::clone(&store);
        let ids = ids.clone();
        handles.push(thread::spawn(move || {
            for round in 0..50u8 {
                for (i, id) in ids.iter().enumerate() {
                    let payload = vec![(t as u8) ^ round ^ (i as u8); 256];
                    store.put(id, &mut &payload[..]).unwrap();
                    if round % 3 == 0 {
                        store.delete(id).unwrap();
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // Run GC while overlapping with a final wave of writes so the mutation
    // lock and Arc-backed readers are exercised.
    let final_store = Arc::clone(&store);
    let ids_for_gc = ids.clone();
    let gc_handle = thread::spawn(move || {
        for id in &ids_for_gc {
            final_store.delete(id).unwrap();
        }
        final_store.force_gc().unwrap();
    });

    for id in &ids {
        let payload = vec![0xABu8; 512];
        store.put(id, &mut &payload[..]).unwrap();
    }
    gc_handle.join().unwrap();

    // Remember which IDs are present before reopen.
    let mut expected = std::collections::HashMap::new();
    for id in &ids {
        if let Some(size) = store.size(id).unwrap() {
            expected.insert(id.clone(), size);
        }
    }

    drop(store);
    let store = BlobStoreImpl::open(dir.path(), opts).unwrap();

    for (id, expected_size) in &expected {
        let actual_size = store.size(id).unwrap();
        assert_eq!(actual_size, Some(*expected_size), "size mismatch for {id:?}");
        let mut reader = store.get(id).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();
        assert_eq!(buf.len() as u64, *expected_size);
    }
}

#[test]
fn reader_holds_volume_alive_during_gc() {
    let opts = BlobStoreOptions {
        max_volume_size: 64 * 1024,
        gc_dead_ratio_threshold: 0.1,
        background_gc: false,
        background_gc_interval: std::time::Duration::from_secs(1),
        sync_on_put: true,
    };
    let (_dir, store) = open(opts);

    // Fill volume 1.
    store.put(b"keep", &mut &vec![1u8; 1024][..]).unwrap();
    store
        .put(b"delete", &mut &vec![2u8; 50 * 1024][..])
        .unwrap();
    // Rotate to volume 2.
    store
        .put(b"rotate", &mut &vec![3u8; 20 * 1024][..])
        .unwrap();

    // Open a reader and only read the first few bytes.
    let mut reader = store.get(b"keep").unwrap();
    let mut prefix = [0u8; 100];
    reader.read_exact(&mut prefix).unwrap();

    // Delete the large object and GC; the old volume must be unlinked.
    store.delete(b"delete").unwrap();
    store.force_gc().unwrap();

    // The reader still has an Arc to the old VolumeReader, so the rest of the
    // payload can still be read even though the directory entry is gone.
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert_eq!(rest.len(), 1024 - 100);
    assert!(rest.iter().all(|&b| b == 1));

    // New reads use the rewritten location.
    let mut new_reader = store.get(b"keep").unwrap();
    let mut new_buf = Vec::new();
    new_reader.read_to_end(&mut new_buf).unwrap();
    assert_eq!(new_buf, vec![1u8; 1024]);
}

#[test]
fn directory_fsync_survives_reopen_after_gc() {
    let opts = BlobStoreOptions {
        max_volume_size: 64 * 1024,
        gc_dead_ratio_threshold: 0.1,
        background_gc: false,
        background_gc_interval: std::time::Duration::from_secs(1),
        sync_on_put: true,
    };
    let (dir, store) = open(opts);

    store.put(b"keep", &mut &vec![1u8; 4096][..]).unwrap();
    store
        .put(b"delete", &mut &vec![2u8; 50 * 1024][..])
        .unwrap();
    store
        .put(b"rotate", &mut &vec![3u8; 20 * 1024][..])
        .unwrap();
    store.delete(b"delete").unwrap();
    store.force_gc().unwrap();
    store.sync().unwrap();
    drop(store);

    let store = BlobStoreImpl::open(dir.path(), BlobStoreOptions::default()).unwrap();
    let mut reader = store.get(b"keep").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, vec![1u8; 4096]);
}
