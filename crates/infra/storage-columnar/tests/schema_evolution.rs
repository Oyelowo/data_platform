use bytes::Bytes;
use storage_columnar::{ColumnarEngineImpl, ColumnarOptions};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn add_nullable_column_in_second_ingest() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();

    engine
        .ingest(vec![
            ("a".into(), vec![Some(Bytes::from("r1")), Some(Bytes::from("r2"))]),
            ("b".into(), vec![Some(Bytes::from("10")), Some(Bytes::from("20"))]),
        ])
        .unwrap();

    engine
        .ingest(vec![
            ("a".into(), vec![Some(Bytes::from("r3")), Some(Bytes::from("r4"))]),
            ("b".into(), vec![Some(Bytes::from("30")), Some(Bytes::from("40"))]),
            ("c".into(), vec![Some(Bytes::from("x")), Some(Bytes::from("y"))]),
        ])
        .unwrap();

    let result = engine.scan(&["a", "b", "c"], &Predicate::True).unwrap();
    let map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
        result.into_iter().collect();

    assert_eq!(
        map["a"],
        vec![
            Some(Bytes::from("r1")),
            Some(Bytes::from("r2")),
            Some(Bytes::from("r3")),
            Some(Bytes::from("r4"))
        ]
    );
    assert_eq!(
        map["b"],
        vec![
            Some(Bytes::from("10")),
            Some(Bytes::from("20")),
            Some(Bytes::from("30")),
            Some(Bytes::from("40"))
        ]
    );
    // Old rows should report NULL for the new column.
    assert_eq!(map["c"][0], None);
    assert_eq!(map["c"][1], None);
    assert_eq!(map["c"][2], Some(Bytes::from("x")));
    assert_eq!(map["c"][3], Some(Bytes::from("y")));
}
