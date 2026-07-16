//! Column-family integration tests.

use storage_kv::{LsmEngine, LsmOptions};

fn small_opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 128, // tiny to force flushes quickly
        max_write_buffer_number: 2,
        ..Default::default()
    }
}

#[test]
fn cf_isolated_from_default() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();

    let cf = engine.create_column_family("cf1", small_opts()).unwrap();

    engine.put(b"key", b"default-value").unwrap();
    engine.put_cf(&cf, b"key", b"cf-value").unwrap();

    assert_eq!(
        engine.get(b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"default-value"))
    );
    assert_eq!(
        engine.get_cf(&cf, b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"cf-value"))
    );

    engine.delete_cf(&cf, b"key").unwrap();
    assert_eq!(engine.get_cf(&cf, b"key").unwrap(), None);
    assert_eq!(
        engine.get(b"key").unwrap(),
        Some(bytes::Bytes::from_static(b"default-value"))
    );
}

#[test]
fn cf_flush_and_recover() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
        let cf = engine.create_column_family("cf1", small_opts()).unwrap();
        // Write enough data to force at least one background flush.
        for i in 0..50u8 {
            engine.put_cf(&cf, &[i], &[i + 100]).unwrap();
        }
        engine.sync().unwrap();

        // Data should be readable from disk.
        for i in 0..50u8 {
            assert_eq!(
                engine.get_cf(&cf, &[i]).unwrap(),
                Some(bytes::Bytes::from(vec![i + 100]))
            );
        }
    }

    // Reopen and recover the column family data from WAL + SSTables.
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
    let cf = engine.cf_handle("cf1").expect("cf1 missing after recovery");
    for i in 0..50u8 {
        assert_eq!(
            engine.get_cf(&cf, &[i]).unwrap(),
            Some(bytes::Bytes::from(vec![i + 100]))
        );
    }
}

#[test]
fn cf_scan_range() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
    let cf = engine.create_column_family("cf1", small_opts()).unwrap();

    for i in 0..10u8 {
        engine.put_cf(&cf, &[i], &[i + 10]).unwrap();
    }

    let mut cursor = engine.scan_cf(&cf, Some(&[2]), Some(&[7])).unwrap();
    let mut results = Vec::new();
    while let Some(Ok((k, v))) = cursor.next() {
        results.push((k[0], v[0]));
    }
    assert_eq!(results, vec![(2, 12), (3, 13), (4, 14), (5, 15), (6, 16)]);
}

#[test]
fn cannot_drop_default_cf() {
    let dir = tempfile::tempdir().unwrap();
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
    let default = engine.cf_handle("default").unwrap();
    assert!(engine.drop_column_family(&default).is_err());
}

#[test]
fn drop_cf_removes_its_files() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
        let cf = engine.create_column_family("cf1", small_opts()).unwrap();
        for i in 0..50u8 {
            engine.put_cf(&cf, &[i], &[i + 100]).unwrap();
        }
        engine.sync().unwrap();
        engine.drop_column_family(&cf).unwrap();
        engine.sync().unwrap();
    }

    // After dropping the CF and syncing, no file belonging to cf1 should remain.
    // Because file numbers are unique, we simply assert there are no SSTables
    // at all in the directory (the default CF had no writes).
    let sst_count = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sst"))
        .count();
    assert_eq!(sst_count, 0, "dropped CF left SSTable files behind");

    // Reopening should not resurrect the column family.
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
    assert!(engine.cf_handle("cf1").is_none());
}

#[test]
fn drop_cf_does_not_delete_other_cfs_files() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
        let cf1 = engine.create_column_family("cf1", small_opts()).unwrap();
        let cf2 = engine.create_column_family("cf2", small_opts()).unwrap();
        for i in 0..50u8 {
            engine.put_cf(&cf1, &[i], &[i + 1]).unwrap();
            engine.put_cf(&cf2, &[i], &[i + 2]).unwrap();
        }
        engine.sync().unwrap();
        engine.drop_column_family(&cf1).unwrap();
        engine.sync().unwrap();

        // cf2 must still be readable.
        for i in 0..50u8 {
            assert_eq!(
                engine.get_cf(&cf2, &[i]).unwrap(),
                Some(bytes::Bytes::from(vec![i + 2]))
            );
        }
    }

    // Reopen: cf1 gone, cf2 present and consistent.
    let engine = LsmEngine::open(dir.path(), small_opts()).unwrap();
    assert!(engine.cf_handle("cf1").is_none());
    let cf2 = engine.cf_handle("cf2").expect("cf2 missing after recovery");
    for i in 0..50u8 {
        assert_eq!(
            engine.get_cf(&cf2, &[i]).unwrap(),
            Some(bytes::Bytes::from(vec![i + 2]))
        );
    }
}
