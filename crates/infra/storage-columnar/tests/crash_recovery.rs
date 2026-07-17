use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn torn_wal_tail_is_truncated_and_earlier_data_survives() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
        engine
            .set_schema(TableSchema {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                }],
            })
            .unwrap();
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from("1"))])])
            .unwrap();
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from("2"))])])
            .unwrap();
        engine.sync().unwrap();
    }

    // Truncate the last WAL segment to simulate a torn write.
    let wal_dir = dir.path().join("manifest-wal");
    let mut segments: Vec<_> = std::fs::read_dir(&wal_dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("wal-") {
                Some(e.path())
            } else {
                None
            }
        })
        .collect();
    segments.sort();
    if let Some(last) = segments.last() {
        let len = std::fs::metadata(last).unwrap().len();
        if len > 20 {
            let file = std::fs::OpenOptions::new().write(true).open(last).unwrap();
            file.set_len(len - 10).unwrap();
        }
    }

    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    // The first record should survive; the torn second record may or may not.
    assert!(result[0].1.contains(&Some(Bytes::from("1"))));
}

#[test]
fn missing_referenced_parquet_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
        engine
            .set_schema(TableSchema {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                }],
            })
            .unwrap();
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from("1"))])])
            .unwrap();
        engine.sync().unwrap();
    }

    // Delete the parquet file from the default partition.
    let default_dir = dir.path().join("__default");
    for entry in std::fs::read_dir(&default_dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".parquet") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }

    let result = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default());
    assert!(result.is_err(), "missing referenced file should fail recovery");
}

#[test]
fn tmp_files_are_cleaned_on_open() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
        engine
            .set_schema(TableSchema {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                }],
            })
            .unwrap();
        // Drop without ingesting so tmp is empty.
        drop(engine);
    }

    let tmp_dir = dir.path().join("tmp");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let stale = tmp_dir.join("orphan.parquet.tmp");
    std::fs::write(&stale, b"junk").unwrap();

    let _engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    assert!(!stale.exists(), "stale temp file should be removed on open");
}
