use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

fn make_engine() -> (tempfile::TempDir, ColumnarEngineImpl) {
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
                    name: "name".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
            ],
        })
        .unwrap();
    (dir, engine)
}

#[test]
fn range_predicate_prunes_files() {
    let (_dir, engine) = make_engine();

    engine
        .ingest(vec![
            ("id".into(), vec![Some(Bytes::from("1")), Some(Bytes::from("2"))]),
            ("name".into(), vec![Some(Bytes::from("a")), Some(Bytes::from("b"))]),
        ])
        .unwrap();

    engine
        .ingest(vec![
            ("id".into(), vec![Some(Bytes::from("10")), Some(Bytes::from("20"))]),
            ("name".into(), vec![Some(Bytes::from("x")), Some(Bytes::from("y"))]),
        ])
        .unwrap();

    let predicate = Predicate::Range {
        column: "id".into(),
        lower: Some(Bytes::from("5")),
        lower_inclusive: true,
        upper: Some(Bytes::from("15")),
        upper_inclusive: true,
    };

    let result = engine.scan(&["id", "name"], &predicate).unwrap();
    assert_eq!(
        engine.file_reads(),
        1,
        "only the file with id 10..=20 should be read"
    );

    let result_map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
        result.into_iter().collect();
    assert_eq!(result_map["id"], vec![Some(Bytes::from("10"))]);
    assert_eq!(result_map["name"], vec![Some(Bytes::from("x"))]);
}

#[test]
fn equality_predicate_with_residual_filtering() {
    let (_dir, engine) = make_engine();

    engine
        .ingest(vec![
            ("id".into(), vec![Some(Bytes::from("1")), Some(Bytes::from("2"))]),
            ("name".into(), vec![Some(Bytes::from("a")), Some(Bytes::from("b"))]),
        ])
        .unwrap();

    engine
        .ingest(vec![
            ("id".into(), vec![Some(Bytes::from("2")), Some(Bytes::from("3"))]),
            ("name".into(), vec![Some(Bytes::from("c")), Some(Bytes::from("d"))]),
        ])
        .unwrap();

    let predicate = Predicate::Eq {
        column: "id".into(),
        value: Bytes::from("2"),
    };

    let result = engine.scan(&["id", "name"], &predicate).unwrap();
    let result_map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
        result.into_iter().collect();
    assert_eq!(
        result_map["id"],
        vec![Some(Bytes::from("2")), Some(Bytes::from("2"))]
    );
    assert_eq!(
        result_map["name"],
        vec![Some(Bytes::from("b")), Some(Bytes::from("c"))]
    );
}
