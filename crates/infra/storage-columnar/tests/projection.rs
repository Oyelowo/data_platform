use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::ColumnarEngine;

#[test]
fn projection_returns_only_requested_columns() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();

    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "a".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
                ColumnDef {
                    name: "b".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "c".into(),
                    ty: ColumnType::Float64,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            ("a".into(), vec![Some(Bytes::from("x")), Some(Bytes::from("y"))]),
            ("b".into(), vec![Some(Bytes::from("1")), Some(Bytes::from("2"))]),
            ("c".into(), vec![Some(Bytes::from("9.9")), Some(Bytes::from("8.8"))]),
        ])
        .unwrap();

    let result = engine
        .scan(&["c", "a"], &storage_traits::Predicate::True)
        .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, "c");
    assert_eq!(
        result[0].1,
        vec![Some(Bytes::from("9.9")), Some(Bytes::from("8.8"))]
    );
    assert_eq!(result[1].0, "a");
    assert_eq!(
        result[1].1,
        vec![Some(Bytes::from("x")), Some(Bytes::from("y"))]
    );
}
