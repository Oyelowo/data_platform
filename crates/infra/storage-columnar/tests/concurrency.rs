use bytes::Bytes;
use std::sync::Arc;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::{ColumnarEngine, Predicate};

#[test]
fn concurrent_ingest_and_scan_are_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap());
    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "value".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    let mut handles = Vec::new();

    for t in 0..4 {
        let engine = Arc::clone(&engine);
        handles.push(std::thread::spawn(move || {
            for i in 0..25 {
                let id = (t * 25 + i).to_string();
                engine
                    .ingest(vec![
                        ("id".into(), vec![Some(Bytes::from(id.clone()))]),
                        ("value".into(), vec![Some(Bytes::from(format!("v-{t}-{i}")))]),
                    ])
                    .unwrap();
            }
        }));
    }

    // While ingesting, run scans. Each scan must observe a consistent snapshot.
    for _ in 0..10 {
        let engine = Arc::clone(&engine);
        handles.push(std::thread::spawn(move || {
            for _ in 0..10 {
                let result = engine.scan(&["id", "value"], &Predicate::True).unwrap();
                let map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
                    result.into_iter().collect();
                let count = map["id"].len();
                assert_eq!(map["value"].len(), count);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let result = engine.scan(&["id"], &Predicate::True).unwrap();
    assert_eq!(result[0].1.len(), 100);
}
