//! Integration tests for storage-wal.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use storage_wal::{Durability, Wal, WalOptions};

#[test]
fn reopen_and_recover_records() {
    let dir = tempfile::tempdir().unwrap();
    let lsns = {
        let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
        let a = wal.append(&b"first"[..], Durability::Immediate).unwrap();
        let b = wal.append(&b"second"[..], Durability::Immediate).unwrap();
        wal.close().unwrap();
        vec![a, b]
    };

    let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
    let first = wal.reader().read(lsns[0]).unwrap().unwrap();
    assert_eq!(first.payload, &b"first"[..]);

    let second = wal.reader().read(lsns[1]).unwrap().unwrap();
    assert_eq!(second.payload, &b"second"[..]);

    let all: Vec<_> = wal
        .iter(0)
        .unwrap()
        .map(|r| r.unwrap().payload)
        .collect();
    assert_eq!(all.len(), 2);
    wal.close().unwrap();
}

#[test]
fn segment_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let opts = WalOptions {
        segment_size: 256, // tiny segments to force rotation
        durability: Durability::Immediate,
    };
    let wal = Wal::open(dir.path(), opts).unwrap();

    // Each record is ~30 bytes; 20 records should span several 256-byte segments.
    let mut lsns = Vec::new();
    for i in 0..20u8 {
        let lsn = wal.append(vec![i; 16], Durability::Immediate).unwrap();
        lsns.push(lsn);
    }

    for (i, lsn) in lsns.iter().enumerate() {
        let rec = wal.reader().read(*lsn).unwrap().unwrap();
        assert_eq!(rec.payload, vec![i as u8; 16]);
    }

    let segments: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(
        segments.len() > 1,
        "expected multiple segments, got {}",
        segments.len()
    );

    wal.close().unwrap();
}

#[test]
fn concurrent_group_commit() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Arc::new(Wal::open(dir.path(), WalOptions::default()).unwrap());
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    for t in 0..8 {
        let wal = wal.clone();
        let counter = counter.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..50usize {
                let payload = format!("t{}-i{}", t, i);
                wal.append(payload.into_bytes(), Durability::Immediate)
                    .unwrap();
                counter.fetch_add(1, Ordering::Relaxed);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::Relaxed), 8 * 50);

    let recovered: Vec<_> = wal
        .iter(0)
        .unwrap()
        .map(|r| String::from_utf8(r.unwrap().payload.to_vec()).unwrap())
        .collect();
    assert_eq!(recovered.len(), 8 * 50);

    Arc::try_unwrap(wal)
        .expect("all other references dropped")
        .close()
        .unwrap();
}

#[test]
fn truncate_before_lsn() {
    let dir = tempfile::tempdir().unwrap();
    let opts = WalOptions {
        segment_size: 128,
        durability: Durability::Immediate,
    };
    let wal = Wal::open(dir.path(), opts).unwrap();

    let checkpoint_lsn = wal.checkpoint(&b"trim-here"[..]).unwrap();
    wal.append(&b"after"[..], Durability::Immediate).unwrap();

    wal.truncate_before(checkpoint_lsn).unwrap();

    // Old segments should be gone but the checkpoint-containing segment must
    // remain because truncation is by first-LSN < before_lsn.
    let segments: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!segments.is_empty());

    wal.close().unwrap();
}

#[test]
fn torn_write_detected_as_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
    wal.append(&b"complete"[..], Durability::Immediate).unwrap();
    wal.close().unwrap();

    // Append garbage to simulate a torn write after a crash.
    let segments: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    assert!(!segments.is_empty());
    let target = &segments[0];
    std::fs::OpenOptions::new()
        .append(true)
        .open(target)
        .unwrap()
        .write_all(&[0xFF, 0xFF])
        .unwrap();

    // Iteration should stop at the valid record without panicking.
    let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
    let mut count = 0;
    for item in wal.iter(0).unwrap() {
        match item {
            Ok(_) => count += 1,
            Err(_) => break,
        }
    }
    assert_eq!(count, 1);
    wal.close().unwrap();
}

#[test]
fn reopen_truncates_torn_tail() {
    let dir = tempfile::tempdir().unwrap();
    let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
    wal.append(&b"before-crash"[..], Durability::Immediate).unwrap();
    wal.close().unwrap();

    // Append trailing garbage to simulate a crash mid-write.
    let segments: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&segments[0])
        .unwrap()
        .write_all(&[0x57, 0xA1, 0x00, 0x01, 0xFF, 0xFF])
        .unwrap();

    // Reopen should recover and allow further valid appends.
    let wal = Wal::open(dir.path(), WalOptions::default()).unwrap();
    let recovered: Vec<_> = wal
        .iter(0)
        .unwrap()
        .map(|r| r.unwrap().payload)
        .collect();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0], &b"before-crash"[..]);

    wal.append(&b"after-crash"[..], Durability::Immediate).unwrap();
    let after = wal
        .iter(0)
        .unwrap()
        .map(|r| r.unwrap().payload)
        .collect::<Vec<_>>();
    assert_eq!(after.len(), 2);
    wal.close().unwrap();
}
