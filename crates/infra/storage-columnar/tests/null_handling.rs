use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn none_and_empty_are_distinct_for_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "s".into(),
                ty: ColumnType::Utf8,
                nullable: true,
            }],
        })
        .unwrap();

    engine
        .ingest(vec![(
            "s".into(),
            vec![None, Some(Bytes::new()), Some(Bytes::from("hello"))],
        )])
        .unwrap();

    let result = engine.scan(&["s"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 3);
    assert_eq!(result[0].1[0], None);
    assert_eq!(result[0].1[1], Some(Bytes::new()));
    assert_eq!(result[0].1[2], Some(Bytes::from("hello")));
}

#[test]
fn none_and_empty_are_distinct_for_binary() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "b".into(),
                ty: ColumnType::Binary,
                nullable: true,
            }],
        })
        .unwrap();

    engine
        .ingest(vec![(
            "b".into(),
            vec![None, Some(Bytes::new()), Some(Bytes::from(&[1u8, 2][..]))],
        )])
        .unwrap();

    let result = engine.scan(&["b"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 3);
    assert_eq!(result[0].1[0], None);
    assert_eq!(result[0].1[1], Some(Bytes::new()));
    assert_eq!(result[0].1[2], Some(Bytes::from(&[1u8, 2][..])));
}

#[test]
fn non_nullable_column_rejects_null() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "id".into(),
                ty: ColumnType::Int64,
                nullable: false,
            }],
        })
        .unwrap();

    let err = engine
        .ingest(vec![("id".into(), vec![Some(Bytes::from("1")), None])])
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("non-nullable column 'id' contains null"),
        "unexpected error: {err}"
    );
}

#[test]
fn non_nullable_column_rejects_missing_column() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![ColumnDef {
                name: "id".into(),
                ty: ColumnType::Int64,
                nullable: false,
            }],
        })
        .unwrap();

    let err = engine
        .ingest(vec![("other".into(), vec![Some(Bytes::from("x"))])])
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("non-nullable column 'id' is missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn null_values_round_trip_for_all_types() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "b".into(),
                    ty: ColumnType::Bool,
                    nullable: true,
                },
                ColumnDef {
                    name: "i".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "f".into(),
                    ty: ColumnType::Float64,
                    nullable: true,
                },
                ColumnDef {
                    name: "s".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
                ColumnDef {
                    name: "bin".into(),
                    ty: ColumnType::Binary,
                    nullable: true,
                },
                ColumnDef {
                    name: "ts".into(),
                    ty: ColumnType::TimestampMicros,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            ("b".into(), vec![None]),
            ("i".into(), vec![None]),
            ("f".into(), vec![None]),
            ("s".into(), vec![None]),
            ("bin".into(), vec![None]),
            ("ts".into(), vec![None]),
        ])
        .unwrap();

    let result = engine
        .scan(&["b", "i", "f", "s", "bin", "ts"], &Predicate::True)
        .unwrap();
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    for col in ["b", "i", "f", "s", "bin", "ts"] {
        assert_eq!(map[col], vec![None], "column {col} did not round-trip null");
    }
}
