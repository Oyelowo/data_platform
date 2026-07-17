//! Recovery behavior when a compaction is interrupted.

use std::fs;
use std::path::Path;

use storage_kv::{LsmEngine, LsmOptions};

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 256,
        max_bytes_for_level_base: 256,
        ..Default::default()
    }
}

fn write_orphan_sstable(db_path: &Path, file_number: u64) {
    // A partially written SSTable: just enough bytes to look like a file,
    // but missing a valid footer / magic number so recovery must ignore it.
    fs::write(db_path.join(format!("{:06}.sst", file_number)), b"orphan").unwrap();
}

/// Orphan SSTables left behind by a crashed compaction are not visible to the
/// recovered engine and are eventually deleted.
#[test]
fn orphan_compaction_output_is_ignored_on_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();

    // Write some data and close cleanly so the manifest and WAL are durable.
    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..50u8 {
            engine.put(&[i], &[i]).unwrap();
        }
        engine.sync().unwrap();
    }

    // Simulate a crash after the compaction wrote an output file but before it
    // logged the manifest edit.  The file is not referenced by the current
    // Version and must be ignored.
    let orphan = 999u64;
    write_orphan_sstable(dir.path(), orphan);
    assert!(dir.path().join(format!("{:06}.sst", orphan)).exists());

    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for i in 0..50u8 {
            assert_eq!(engine.get(&[i]).unwrap(), Some(bytes::Bytes::from(vec![i])));
        }
        // The orphan file should have been cleaned up during open.
        assert!(!dir.path().join(format!("{:06}.sst", orphan)).exists());
    }
}

/// Reopening after many compactions returns the exact same data as before the
/// final close.
#[test]
fn reopen_after_compactions_is_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let opts = opts();

    let mut expected = std::collections::BTreeMap::new();

    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        // Interleave puts and deletes with enough volume to trigger many
        // flushes and compactions.
        for round in 0..20u8 {
            for i in 0..100u8 {
                engine.put(&[i], &[round, i]).unwrap();
            }
            if round % 3 == 0 {
                for i in (0..100u8).step_by(5) {
                    engine.delete(&[i]).unwrap();
                }
            }
        }
        engine.sync().unwrap();
    }

    {
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        // Round 19 overwrites every key, including those deleted at round 18.
        for i in 0..100u8 {
            expected.insert(i, bytes::Bytes::from(vec![19, i]));
            assert_eq!(engine.get(&[i]).unwrap(), Some(expected[&i].clone()));
        }
    }
}
