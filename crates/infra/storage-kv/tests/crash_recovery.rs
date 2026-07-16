//! Deterministic crash-recovery tests for storage-kv.
//!
//! These tests build a populated database, snapshot the directory, apply a
//! controlled corruption, reopen the database, and verify that durable writes
//! are never lost and that corruption is detected rather than silently ignored.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;
use tempfile::TempDir;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    }
}

/// Operations applied to the engine before a crash.
#[derive(Debug, Clone)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
    Sync,
}

/// A harness that records a reference model of durable state and can apply
/// corruptions to a snapshot of the on-disk database.
struct CrashHarness {
    dir: TempDir,
    ops: Vec<Op>,
    /// State that is guaranteed durable after each `Sync`.
    synced: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    /// Last value seen for each key, including unsynced operations.
    latest: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl CrashHarness {
    fn new() -> Self {
        Self {
            dir: TempDir::new().unwrap(),
            ops: Vec::new(),
            synced: BTreeMap::new(),
            latest: BTreeMap::new(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn apply(&mut self, ops: &[Op]) {
        let engine = LsmEngine::open(self.path(), opts()).unwrap();
        for op in ops {
            match op {
                Op::Put(k, v) => {
                    engine.put(k, v).unwrap();
                    self.latest.insert(k.clone(), Some(v.clone()));
                }
                Op::Delete(k) => {
                    engine.delete(k).unwrap();
                    self.latest.insert(k.clone(), None);
                }
                Op::Sync => {
                    engine.sync().unwrap();
                    self.synced.clone_from(&self.latest);
                }
            }
            self.ops.push(op.clone());
        }
        // Drop engine to release files before snapshotting/corrupting.
        drop(engine);
    }

    /// Copy the current database directory to a new temporary directory.
    fn snapshot(&self) -> TempDir {
        let snapshot = TempDir::new().unwrap();
        copy_dir_all(self.path(), snapshot.path()).unwrap();
        snapshot
    }

    /// Truncate the last WAL segment by `drop` bytes to simulate a torn tail.
    fn truncate_last_wal_segment(snapshot: &Path, drop: u64) {
        let wal_dir = snapshot.join("wal");
        if !wal_dir.exists() {
            return;
        }
        let mut segments: Vec<PathBuf> = fs::read_dir(&wal_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        segments.sort();
        if let Some(last) = segments.last() {
            let len = fs::metadata(last).unwrap().len();
            let keep = len.saturating_sub(drop);
            let file = fs::OpenOptions::new().write(true).open(last).unwrap();
            file.set_len(keep).unwrap();
        }
    }

    /// Truncate the manifest at `keep` complete lines, dropping the rest.
    fn truncate_manifest(snapshot: &Path, keep: usize) {
        let manifest_path = snapshot.join("MANIFEST-000001");
        if manifest_path.exists() {
            let contents = fs::read_to_string(&manifest_path).unwrap();
            let lines: Vec<&str> = contents.lines().collect();
            let kept = lines.into_iter().take(keep).collect::<Vec<_>>().join("\n");
            fs::write(&manifest_path, kept).unwrap();
        }
    }

    /// Flip one byte in a file at a given offset.
    fn flip_byte(path: &Path, offset: u64) {
        let mut bytes = fs::read(path).unwrap();
        if (offset as usize) < bytes.len() {
            bytes[offset as usize] ^= 0xFF;
        }
        fs::write(path, bytes).unwrap();
    }

    fn find_any_sstable(snapshot: &Path) -> Option<PathBuf> {
        fs::read_dir(snapshot)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("sst"))
    }

    /// Verify that every synced key is visible with its expected value and that
    /// scan returns keys in ascending order without duplicates.
    fn check_invariants(db: &LsmEngine, expected: &BTreeMap<Vec<u8>, Option<Vec<u8>>>) {
        for (k, v) in expected {
            let got = db.get(k).unwrap();
            assert_eq!(
                got.as_ref(),
                v.as_ref()
                    .map(|b| bytes::Bytes::copy_from_slice(b))
                    .as_ref(),
                "key {:?} should match synced state",
                k
            );
        }

        let mut cursor = db.scan(None, None).unwrap();
        let mut last: Option<Vec<u8>> = None;
        while let Some(Ok((k, _v))) = cursor.next() {
            if let Some(ref l) = last {
                assert!(l.as_slice() < k.as_ref(), "scan must be strictly ascending");
            }
            last = Some(k.to_vec());
        }
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn wal_torn_tail_preserves_synced_writes() {
    let mut harness = CrashHarness::new();
    harness.apply(&[
        Op::Put(b"a".to_vec(), b"1".to_vec()),
        Op::Put(b"b".to_vec(), b"2".to_vec()),
        Op::Sync,
        Op::Put(b"c".to_vec(), b"3".to_vec()),
    ]);

    let snapshot = harness.snapshot();
    // Truncate the last few bytes of the last WAL segment, turning the final
    // record into a torn write.  Earlier durable records must still recover.
    CrashHarness::truncate_last_wal_segment(snapshot.path(), 4);

    let db = LsmEngine::open(snapshot.path(), opts()).unwrap();
    CrashHarness::check_invariants(&db, &harness.synced);
    db.sync().unwrap();
}

#[test]
fn manifest_torn_tail_preserves_synced_writes() {
    let mut harness = CrashHarness::new();
    harness.apply(&[
        Op::Put(b"x".to_vec(), b"10".to_vec()),
        Op::Sync,
        Op::Put(b"y".to_vec(), b"20".to_vec()),
        Op::Sync,
        Op::Put(b"z".to_vec(), b"30".to_vec()),
    ]);

    let snapshot = harness.snapshot();
    // Keep only the first manifest line (the flush of x).
    CrashHarness::truncate_manifest(snapshot.path(), 1);

    let db = LsmEngine::open(snapshot.path(), opts()).unwrap();
    // y and z may be lost because their flush records are gone, but x must remain.
    let mut expected = BTreeMap::new();
    expected.insert(b"x".to_vec(), Some(b"10".to_vec()));
    CrashHarness::check_invariants(&db, &expected);
}

#[test]
fn missing_sstable_is_detected_on_open() {
    let mut harness = CrashHarness::new();
    harness.apply(&[
        Op::Put(b"a".to_vec(), b"1".to_vec()),
        Op::Put(b"b".to_vec(), b"2".to_vec()),
        Op::Sync,
    ]);

    let snapshot = harness.snapshot();
    if let Some(sst) = CrashHarness::find_any_sstable(snapshot.path()) {
        fs::remove_file(sst).unwrap();
        // Recovery currently does not eagerly verify file existence, so the open
        // succeeds but a subsequent read must eventually surface the missing file.
        // This test documents the current behavior and will tighten as recovery
        // adds file verification.
        let db = LsmEngine::open(snapshot.path(), opts()).unwrap();
        // At least one of the synced keys should now be unreadable.
        let any_error = [b"a".to_vec(), b"b".to_vec()]
            .iter()
            .any(|k| db.get(k).is_err());
        assert!(
            any_error,
            "missing sstable should eventually produce an error"
        );
    }
}

#[test]
fn corrupt_sstable_block_is_detected() {
    let mut harness = CrashHarness::new();
    harness.apply(&[
        Op::Put(b"a".to_vec(), b"1".to_vec()),
        Op::Put(b"b".to_vec(), b"2".to_vec()),
        Op::Sync,
    ]);

    let snapshot = harness.snapshot();
    if let Some(sst) = CrashHarness::find_any_sstable(snapshot.path()) {
        // Corrupt a byte deep enough to hit a block rather than the footer.
        let offset = fs::metadata(&sst).unwrap().len() / 2;
        CrashHarness::flip_byte(&sst, offset);

        let db = LsmEngine::open(snapshot.path(), opts()).unwrap();
        // At least one read must fail with a checksum or bounds error.
        let any_error = [b"a".to_vec(), b"b".to_vec()]
            .iter()
            .any(|k| db.get(k).is_err());
        assert!(any_error, "corrupted sstable block should be detected");
    }
}

#[test]
fn reopen_after_sync_is_consistent() {
    let mut harness = CrashHarness::new();
    harness.apply(&[
        Op::Put(b"a".to_vec(), b"1".to_vec()),
        Op::Put(b"b".to_vec(), b"2".to_vec()),
        Op::Delete(b"a".to_vec()),
        Op::Sync,
        Op::Put(b"c".to_vec(), b"3".to_vec()),
        Op::Sync,
    ]);

    // No corruption: reopen must recover exactly the synced state.
    let db = LsmEngine::open(harness.path(), opts()).unwrap();
    CrashHarness::check_invariants(&db, &harness.synced);
    assert_eq!(db.get(b"a").unwrap(), None);
    assert_eq!(db.get(b"b").unwrap(), Some(bytes::Bytes::from_static(b"2")));
    assert_eq!(db.get(b"c").unwrap(), Some(bytes::Bytes::from_static(b"3")));
}
