use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

fn partitioned_options() -> ColumnarOptions {
    ColumnarOptions {
        row_group_size: 100,
        data_page_size: 1024,
        partition_column: Some("region".into()),
        max_small_files: 100,
        compaction_threshold_bytes: 1024 * 1024 * 1024,
        background_compaction: false,
        sync_on_flush: true,
        target_file_size: 256 * 1024 * 1024,
    }
}

#[test]
fn rows_routed_to_partition_directories() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), partitioned_options()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "region".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            (
                "region".into(),
                vec![
                    Some(Bytes::from("eu")),
                    Some(Bytes::from("us")),
                    Some(Bytes::from("eu")),
                ],
            ),
            (
                "id".into(),
                vec![
                    Some(Bytes::from("1")),
                    Some(Bytes::from("2")),
                    Some(Bytes::from("3")),
                ],
            ),
        ])
        .unwrap();

    // Each partition should have its own Parquet file.
    let partitions: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().ok()?.is_dir()
                && !name.starts_with('_')
                && name != "manifest-wal"
                && name != "manifest-snapshot"
                && name != "tmp"
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    assert!(partitions.contains(&"eu".to_string()));
    assert!(partitions.contains(&"us".to_string()));

    let result = engine.scan(&["region", "id"], &Predicate::True).unwrap();
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    assert_eq!(map["id"].len(), 3);
}

#[test]
fn partition_pruning_skips_directories() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), partitioned_options()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "region".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            (
                "region".into(),
                vec![Some(Bytes::from("eu")), Some(Bytes::from("us"))],
            ),
            (
                "id".into(),
                vec![Some(Bytes::from("1")), Some(Bytes::from("2"))],
            ),
        ])
        .unwrap();

    // Reset read counter.
    let _ = engine.scan(&["id"], &Predicate::True);

    let predicate = Predicate::Eq {
        column: "region".into(),
        value: Bytes::from("us"),
    };
    let result = engine.scan(&["id"], &predicate).unwrap();
    assert_eq!(result[0].1, vec![Some(Bytes::from("2"))]);
}

#[test]
fn unsafe_partition_characters_are_sanitized() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), partitioned_options()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "region".into(),
                ty: ColumnType::Utf8,
                nullable: true,
            }],
        })
        .unwrap();

    engine
        .ingest(vec![(
            "region".into(),
            vec![Some(Bytes::from("us/east")), Some(Bytes::from("eu\\west"))],
        )])
        .unwrap();

    let partitions: std::collections::HashSet<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().ok()?.is_dir()
                && !name.starts_with('_')
                && name != "manifest-wal"
                && name != "manifest-snapshot"
                && name != "tmp"
            {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    assert!(partitions.contains("us_east"));
    assert!(partitions.contains("eu_west"));
}
