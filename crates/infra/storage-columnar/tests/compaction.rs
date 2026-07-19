use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

fn small_file_options() -> ColumnarOptions {
    ColumnarOptions {
        row_group_size: 10,
        data_page_size: 1024,
        partition_column: None,
        max_small_files: 2,
        compaction_threshold_bytes: 1024 * 1024,
        background_compaction: false,
        sync_on_flush: true,
        target_file_size: 256 * 1024 * 1024,
    }
}

#[test]
fn compaction_reduces_file_count_and_preserves_rows() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), small_file_options()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "name".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    for i in 0..4 {
        engine
            .ingest(vec![
                ("id".into(), vec![Some(Bytes::from(i.to_string()))]),
                ("name".into(), vec![Some(Bytes::from(format!("n{i}")))]),
            ])
            .unwrap();
    }

    assert_eq!(engine.file_count(), 4);
    let removed = engine.force_compaction(None).unwrap();
    assert_eq!(removed, 4);
    assert_eq!(engine.file_count(), 1);

    let result = engine.scan(&["id", "name"], &Predicate::True).unwrap();
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    assert_eq!(map["id"].len(), 4);
    let ids: Vec<i64> = map["id"]
        .iter()
        .map(|v| {
            std::str::from_utf8(v.as_ref().unwrap())
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(ids, vec![0, 1, 2, 3]);
}

#[test]
fn compaction_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = ColumnarEngineImpl::open(dir.path(), small_file_options()).unwrap();
        engine
            .set_schema(TableSchema {
                columns: vec![ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                }],
            })
            .unwrap();
        for i in 0..4 {
            engine
                .ingest(vec![("id".into(), vec![Some(Bytes::from(i.to_string()))])])
                .unwrap();
        }
        engine.force_compaction(None).unwrap();
        engine.sync().unwrap();
    }

    let engine = ColumnarEngineImpl::open(dir.path(), small_file_options()).unwrap();
    assert_eq!(engine.file_count(), 1);
    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 4);
}

#[test]
fn compaction_honors_target_file_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = small_file_options();
    options.target_file_size = 1; // Force splitting regardless of input size.
    options.max_small_files = 2;

    let engine = ColumnarEngineImpl::open(dir.path(), options).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "id".into(),
                ty: ColumnType::Int64,
                nullable: true,
            }],
        })
        .unwrap();

    // Ingest enough rows that splitting produces multiple output files.
    for i in 0..8 {
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from(i.to_string()))])])
            .unwrap();
    }

    assert_eq!(engine.file_count(), 8);
    let removed = engine.force_compaction(None).unwrap();
    assert_eq!(removed, 8);
    assert!(
        engine.file_count() > 1,
        "compaction should split output when target_file_size is tiny"
    );

    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 8);
}

#[test]
fn background_compaction_reduces_file_count() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = small_file_options();
    options.background_compaction = true;
    options.sync_on_flush = false;

    let engine = ColumnarEngineImpl::open(dir.path(), options).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "id".into(),
                ty: ColumnType::Int64,
                nullable: true,
            }],
        })
        .unwrap();

    for i in 0..8 {
        engine
            .ingest(vec![("id".into(), vec![Some(Bytes::from(i.to_string()))])])
            .unwrap();
    }

    // Wait for the background worker to finish compaction. It runs after each
    // ingest trigger, but because it is asynchronous we poll briefly.
    let mut attempts = 0;
    while engine.file_count() > 1 && attempts < 50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        attempts += 1;
    }

    assert_eq!(
        engine.file_count(),
        1,
        "background compaction should eventually coalesce files"
    );

    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 8);
}
