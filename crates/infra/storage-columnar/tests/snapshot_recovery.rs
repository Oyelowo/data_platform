use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn snapshot_and_wal_truncation_are_consistent() {
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
        engine.snapshot().unwrap();
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from("2"))])])
            .unwrap();
        engine.sync().unwrap();
    }

    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    let values: Vec<i64> = result[0]
        .1
        .iter()
        .map(|v| std::str::from_utf8(v.as_ref().unwrap()).unwrap().parse().unwrap())
        .collect();
    assert_eq!(values, vec![1, 2]);
}

#[test]
fn snapshot_survives_missing_wal_segments() {
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
        engine.snapshot().unwrap();
    }

    // Manually remove all WAL segments; the snapshot must still be enough.
    let wal_dir = dir.path().join("manifest-wal");
    if wal_dir.exists() {
        for entry in std::fs::read_dir(&wal_dir).unwrap().flatten() {
            if entry.file_name().to_string_lossy().starts_with("wal-") {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
    }

    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    assert_eq!(result[0].1, vec![Some(Bytes::from("1"))]);
}
