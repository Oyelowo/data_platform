//! Crash-recovery and corruption-detection tests for `storage-blob`.
//!
//! These tests simulate power-loss scenarios by truncating durable files while
//! the store is closed, then verify that reopening recovers a consistent state.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use storage_blob::{BlobStoreImpl, BlobStoreOptions};
use storage_traits::BlobStore;
use storage_wal::{Record, RECORD_HEADER_SIZE};
use tempfile::TempDir;

fn open(dir: &TempDir) -> BlobStoreImpl {
    BlobStoreImpl::open(dir.path(), BlobStoreOptions::default()).unwrap()
}

fn active_volume_path(dir: &Path) -> PathBuf {
    let volumes_dir = dir.join("volumes");
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(&volumes_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".blob") {
            if let Ok(n) = stem.parse::<u64>() {
                let path = entry.path();
                if best.as_ref().map(|(bn, _)| n > *bn).unwrap_or(true) {
                    best = Some((n, path));
                }
            }
        }
    }
    best.expect("no volume files found").1
}

fn wal_segment_path(dir: &Path) -> PathBuf {
    let wal_dir = dir.join("index-wal");
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(&wal_dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(stem) = name.strip_suffix(".log") {
            if let Some(lsn_str) = stem.strip_prefix("wal-") {
                if let Ok(n) = lsn_str.parse::<u64>() {
                    let path = entry.path();
                    if best.as_ref().map(|(bn, _)| n > *bn).unwrap_or(true) {
                        best = Some((n, path));
                    }
                }
            }
        }
    }
    best.expect("no wal segment files found").1
}

/// Return the byte offsets at which each WAL record begins in `path`.
/// Stops at the first partial or corrupt record, mirroring WAL recovery.
fn wal_record_offsets(path: &Path) -> Vec<u64> {
    let bytes = fs::read(path).unwrap();
    let mut offsets = Vec::new();
    let mut cursor: usize = 0;
    while cursor < bytes.len() {
        match Record::decode(&bytes[cursor..]) {
            Ok(Some((_, consumed))) => {
                offsets.push(cursor as u64);
                cursor += consumed;
            }
            Ok(None) | Err(_) => break,
        }
    }
    offsets
}

/// Return the byte offsets at which each volume record begins in `path`.
fn volume_record_offsets(path: &Path) -> Vec<u64> {
    use storage_blob::volume::VolumeReader;

    let reader = VolumeReader::open(path, 0).unwrap();
    let file_size = reader.file_size().unwrap();
    let mut offsets = Vec::new();
    let mut offset = 0u64;
    while offset + storage_blob::format::HEADER_SIZE as u64 <= file_size {
        offsets.push(offset);
        match reader.read_header(offset) {
            Ok((header, _)) => offset += header.record_size(),
            Err(_) => break,
        }
    }
    offsets
}

fn truncate_file(path: &Path, new_len: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(new_len).unwrap();
}

#[test]
fn torn_volume_tail_is_truncated_and_earlier_objects_survive() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);

    store.put(b"safe", &mut &b"safe payload"[..]).unwrap();
    store.put(b"torn", &mut &b"torn payload"[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    // Truncate into the last record to simulate a torn append.
    let volume_path = active_volume_path(dir.path());
    let file_size = fs::metadata(&volume_path).unwrap().len();
    assert!(file_size >= 4, "volume file unexpectedly small");
    truncate_file(&volume_path, file_size - 4);

    let store = open(&dir);
    let mut reader = store.get(b"safe").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"safe payload");

    // The torn object is either missing or unreadable.
    assert!(store.get(b"torn").is_err() || {
        let mut r = store.get(b"torn").unwrap();
        r.read_to_end(&mut Vec::new()).is_err()
    });
}

#[test]
fn truncated_index_wal_drops_later_puts() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);

    store.put(b"first", &mut &b"first payload"[..]).unwrap();
    store.put(b"second", &mut &b"second payload"[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    // Remove the last index record so only `first` is recovered.
    let wal_path = wal_segment_path(dir.path());
    let offsets = wal_record_offsets(&wal_path);
    assert!(offsets.len() >= 2, "expected at least two index wal records");
    let truncate_to = *offsets.last().unwrap();
    truncate_file(&wal_path, truncate_to);

    let store = open(&dir);
    let mut reader = store.get(b"first").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"first payload");

    assert!(store.get(b"second").is_err());
    assert_eq!(store.size(b"second").unwrap(), None);
}

#[test]
fn corrupt_volume_payload_is_detected_on_read() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);

    store.put(b"fragile", &mut &b"fragile payload"[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    let volume_path = active_volume_path(dir.path());
    let offsets = volume_record_offsets(&volume_path);
    assert!(!offsets.is_empty());

    // Flip one payload byte of the only record.
    let mut bytes = fs::read(&volume_path).unwrap();
    let payload_offset = offsets[0] as usize
        + storage_blob::format::HEADER_SIZE as usize
        + b"fragile".len();
    bytes[payload_offset] = bytes[payload_offset].wrapping_add(1);
    fs::write(&volume_path, bytes).unwrap();

    let store = open(&dir);
    let mut reader = store.get(b"fragile").unwrap();
    let err = reader.read_to_end(&mut Vec::new()).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("crc"));
}

#[test]
fn corrupt_wal_record_is_truncated_and_earlier_records_survive() {
    let dir = TempDir::new().unwrap();
    let store = open(&dir);
    store.put(b"first", &mut &b"first payload"[..]).unwrap();
    store.put(b"second", &mut &b"second payload"[..]).unwrap();
    store.sync().unwrap();
    drop(store);

    let wal_path = wal_segment_path(dir.path());
    let offsets = wal_record_offsets(&wal_path);
    assert!(offsets.len() >= 2, "expected at least two index wal records");

    let mut bytes = fs::read(&wal_path).unwrap();
    // Flip a byte inside the payload area of the second record.
    let corrupt_at = offsets[1] as usize + RECORD_HEADER_SIZE + 1;
    assert!(corrupt_at < bytes.len());
    bytes[corrupt_at] = bytes[corrupt_at].wrapping_add(1);
    fs::write(&wal_path, bytes).unwrap();

    let store = open(&dir);
    let mut reader = store.get(b"first").unwrap();
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, b"first payload");

    // The corrupt record is treated as a torn tail and discarded.
    assert!(store.get(b"second").is_err());
    assert_eq!(store.size(b"second").unwrap(), None);
}
