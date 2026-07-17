use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn data_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();

    {
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
        engine
            .ingest(vec![
                (
                    "id".into(),
                    vec![Some(Bytes::from("1")), Some(Bytes::from("2"))],
                ),
                (
                    "name".into(),
                    vec![Some(Bytes::from("a")), Some(Bytes::from("b"))],
                ),
            ])
            .unwrap();
        engine.sync().unwrap();
        // Engine is dropped here, closing the WAL.
    }

    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();
    let result = engine.scan(&["id", "name"], &Predicate::True).unwrap();
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> = result.into_iter().collect();
    assert_eq!(
        map["id"],
        vec![Some(Bytes::from("1")), Some(Bytes::from("2"))]
    );
    assert_eq!(
        map["name"],
        vec![Some(Bytes::from("a")), Some(Bytes::from("b"))]
    );
}
