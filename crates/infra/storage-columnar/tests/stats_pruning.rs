use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn multi_digit_integer_stats_prune_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "payload".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    // File 1: ids 1 and 2 (lexicographically smaller than "10").
    engine
        .ingest(vec![
            (
                "id".into(),
                vec![Some(Bytes::from("1")), Some(Bytes::from("2"))],
            ),
            (
                "payload".into(),
                vec![Some(Bytes::from("a")), Some(Bytes::from("b"))],
            ),
        ])
        .unwrap();

    // File 2: ids 10 and 20.
    engine
        .ingest(vec![
            (
                "id".into(),
                vec![Some(Bytes::from("10")), Some(Bytes::from("20"))],
            ),
            (
                "payload".into(),
                vec![Some(Bytes::from("x")), Some(Bytes::from("y"))],
            ),
        ])
        .unwrap();

    // A range that only includes values >= 5. File 1 (max 2) must be skipped.
    let predicate = Predicate::Range {
        column: "id".into(),
        lower: Some(Bytes::from("5")),
        lower_inclusive: true,
        upper: None,
        upper_inclusive: true,
    };

    let result = engine.scan(&["id", "payload"], &predicate).unwrap();
    assert_eq!(
        engine.file_reads(),
        1,
        "only the file containing id 10/20 should be read"
    );
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    assert_eq!(
        map["id"],
        vec![Some(Bytes::from("10")), Some(Bytes::from("20"))]
    );
}

#[test]
fn float_stats_prune_by_numeric_value() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "value".into(),
                    ty: ColumnType::Float64,
                    nullable: true,
                },
                ColumnDef {
                    name: "label".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            (
                "value".into(),
                vec![Some(Bytes::from("0.5")), Some(Bytes::from("1.5"))],
            ),
            (
                "label".into(),
                vec![Some(Bytes::from("a")), Some(Bytes::from("b"))],
            ),
        ])
        .unwrap();

    engine
        .ingest(vec![
            (
                "value".into(),
                vec![Some(Bytes::from("9.9")), Some(Bytes::from("10.1"))],
            ),
            (
                "label".into(),
                vec![Some(Bytes::from("c")), Some(Bytes::from("d"))],
            ),
        ])
        .unwrap();

    let predicate = Predicate::Range {
        column: "value".into(),
        lower: Some(Bytes::from("5.0")),
        lower_inclusive: true,
        upper: None,
        upper_inclusive: true,
    };

    let result = engine.scan(&["value", "label"], &predicate).unwrap();
    assert_eq!(engine.file_reads(), 1);
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    assert_eq!(
        map["value"],
        vec![Some(Bytes::from("9.9")), Some(Bytes::from("10.1"))]
    );
}
