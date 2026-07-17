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
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
        result.into_iter().collect();
    assert_eq!(map["id"].len(), 4);
    let ids: Vec<i64> = map["id"]
        .iter()
        .map(|v| std::str::from_utf8(v.as_ref().unwrap()).unwrap().parse().unwrap())
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
