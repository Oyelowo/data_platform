//! Integration tests for checkpoints and named backups.

use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

#[test]
fn checkpoint_is_consistent_point_in_time() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"before", b"1").unwrap();
    engine.sync().unwrap();

    engine.checkpoint(checkpoint.path()).unwrap();

    // Writes after the checkpoint must not be visible when the checkpoint is reopened.
    engine.put(b"after", b"2").unwrap();
    engine.sync().unwrap();

    let reopened = LsmEngine::open(checkpoint.path(), LsmOptions::default()).unwrap();
    assert_eq!(reopened.get(b"before").unwrap().unwrap().as_ref(), b"1");
    assert!(reopened.get(b"after").unwrap().is_none());
}

#[test]
fn backup_restore_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let restore = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"k1", b"v1").unwrap();
    engine.put(b"k2", b"v2").unwrap();
    engine.delete(b"k2").unwrap();
    engine.sync().unwrap();

    engine.create_backup("snap1").unwrap();
    engine.restore_backup("snap1", restore.path()).unwrap();

    let restored = LsmEngine::open(restore.path(), LsmOptions::default()).unwrap();
    assert_eq!(restored.get(b"k1").unwrap().unwrap().as_ref(), b"v1");
    assert!(restored.get(b"k2").unwrap().is_none());
}

#[test]
fn backup_survives_live_compaction() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint = tempfile::tempdir().unwrap();
    let mut opts = LsmOptions::default();
    opts.write_buffer_size = 1024;
    opts.target_file_size_base = 1024;
    opts.level0_file_num_compaction_trigger = 2;
    let engine = LsmEngine::open(dir.path(), opts).unwrap();

    // Write enough data to create several L0 files and trigger compaction.
    for i in 0..100u32 {
        engine
            .put(&i.to_be_bytes(), &format!("value-{i}").into_bytes())
            .unwrap();
    }
    engine.sync().unwrap();

    engine.checkpoint(checkpoint.path()).unwrap();

    // Force more writes and compactions on the live engine.  Because SSTables
    // are immutable, the hard-linked checkpoint files must remain valid.
    for i in 100..200u32 {
        engine
            .put(&i.to_be_bytes(), &format!("value-{i}").into_bytes())
            .unwrap();
    }
    engine.sync().unwrap();

    let reopened = LsmEngine::open(checkpoint.path(), LsmOptions::default()).unwrap();
    for i in 0..100u32 {
        let expected = format!("value-{i}");
        assert_eq!(
            reopened.get(&i.to_be_bytes()).unwrap().unwrap().as_ref(),
            expected.as_bytes()
        );
    }
    assert!(reopened.get(&100u32.to_be_bytes()).unwrap().is_none());
}

#[test]
fn multi_column_family_backup() {
    let dir = tempfile::tempdir().unwrap();
    let restore = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    let cf1 = engine
        .create_column_family("cf1", LsmOptions::default())
        .unwrap();
    engine.put_cf(&cf1, b"cf1-key", b"cf1-value").unwrap();
    engine.put(b"default-key", b"default-value").unwrap();
    engine.sync().unwrap();

    engine.create_backup("multi-cf").unwrap();
    engine.restore_backup("multi-cf", restore.path()).unwrap();

    let restored = LsmEngine::open(restore.path(), LsmOptions::default()).unwrap();
    let restored_cf1 = restored.cf_handle("cf1").unwrap();
    assert_eq!(
        restored.get(b"default-key").unwrap().unwrap().as_ref(),
        b"default-value"
    );
    assert_eq!(
        restored
            .get_cf(&restored_cf1, b"cf1-key")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"cf1-value"
    );
}

#[test]
fn backup_with_range_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let restore = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"a", b"1").unwrap();
    engine.put(b"b", b"2").unwrap();
    engine.put(b"c", b"3").unwrap();
    engine.delete_range(b"a", b"c").unwrap();
    engine.sync().unwrap();

    engine.create_backup("range").unwrap();
    engine.restore_backup("range", restore.path()).unwrap();

    let restored = LsmEngine::open(restore.path(), LsmOptions::default()).unwrap();
    assert!(restored.get(b"a").unwrap().is_none());
    assert!(restored.get(b"b").unwrap().is_none());
    assert_eq!(restored.get(b"c").unwrap().unwrap().as_ref(), b"3");
}

#[test]
fn list_and_delete_backups() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.create_backup("first").unwrap();
    engine.create_backup("second").unwrap();

    let mut names = engine.list_backups().unwrap();
    names.sort();
    assert_eq!(names, vec!["first", "second"]);

    engine.delete_backup("first").unwrap();
    assert_eq!(engine.list_backups().unwrap(), vec!["second"]);
}

#[test]
fn restore_refuses_non_empty_target() {
    let dir = tempfile::tempdir().unwrap();
    let restore = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();
    engine.create_backup("snap").unwrap();

    std::fs::write(restore.path().join("spurious.txt"), b"x").unwrap();
    assert!(engine.restore_backup("snap", restore.path()).is_err());
}

#[test]
fn checkpoint_does_not_include_later_writes_in_scan() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"a", b"1").unwrap();
    engine.put(b"b", b"2").unwrap();
    engine.sync().unwrap();

    engine.checkpoint(checkpoint.path()).unwrap();

    engine.put(b"c", b"3").unwrap();
    engine.sync().unwrap();

    let reopened = LsmEngine::open(checkpoint.path(), LsmOptions::default()).unwrap();
    let mut cursor = reopened.scan(None, None).unwrap();
    let mut keys = Vec::new();
    while let Some(item) = cursor.next() {
        let (k, _v) = item.unwrap();
        keys.push(k.to_vec());
    }
    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);
}

#[test]
fn backup_hard_links_sstable_files() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"x", b"y").unwrap();
    engine.sync().unwrap();
    engine.create_backup("link-check").unwrap();

    let backup_dir = dir.path().join("backups").join("link-check");
    let mut engine_sst = None;
    let mut backup_sst = None;

    for entry in std::fs::read_dir(dir.path()).unwrap() {
        let e = entry.unwrap();
        if e.file_name().to_string_lossy().ends_with(".sst") {
            engine_sst = Some(e.path());
        }
    }
    for entry in std::fs::read_dir(&backup_dir).unwrap() {
        let e = entry.unwrap();
        if e.file_name().to_string_lossy().ends_with(".sst") {
            backup_sst = Some(e.path());
        }
    }

    let engine_sst = engine_sst.expect("engine should have an sst");
    let backup_sst = backup_sst.expect("backup should have an sst");

    // Hard-linked files share the same inode on Unix; on other platforms this
    // falls back to a copy, so the assertion is Unix-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&engine_sst).unwrap().ino(),
            std::fs::metadata(&backup_sst).unwrap().ino()
        );
    }

    // In all cases the backup SSTable must be readable and consistent.
    let restored = LsmEngine::open(&backup_dir, LsmOptions::default()).unwrap();
    assert_eq!(restored.get(b"x").unwrap().unwrap().as_ref(), b"y");
}

#[test]
fn checkpoint_preserves_deletes() {
    let dir = tempfile::tempdir().unwrap();
    let checkpoint = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"k", b"v").unwrap();
    engine.sync().unwrap();
    engine.delete(b"k").unwrap();
    engine.sync().unwrap();

    engine.checkpoint(checkpoint.path()).unwrap();

    let reopened = LsmEngine::open(checkpoint.path(), LsmOptions::default()).unwrap();
    assert!(reopened.get(b"k").unwrap().is_none());
}

#[test]
fn restore_is_independent_of_backup() {
    let dir = tempfile::tempdir().unwrap();
    let restore = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), LsmOptions::default()).unwrap();

    engine.put(b"k", b"v1").unwrap();
    engine.sync().unwrap();
    engine.create_backup("independent").unwrap();

    engine
        .restore_backup("independent", restore.path())
        .unwrap();

    // Mutating the restored engine must not affect the backup.
    let restored = LsmEngine::open(restore.path(), LsmOptions::default()).unwrap();
    restored.put(b"k", b"v2").unwrap();
    restored.sync().unwrap();

    let from_backup = LsmEngine::open(
        dir.path().join("backups").join("independent"),
        LsmOptions::default(),
    )
    .unwrap();
    assert_eq!(from_backup.get(b"k").unwrap().unwrap().as_ref(), b"v1");
}
