use bytes::Bytes;
use storage_columnar::{ColumnDef, ColumnType, ColumnarEngineImpl, ColumnarOptions, TableSchema};
use storage_traits::ColumnarEngine;

#[test]
fn basic_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = ColumnarEngineImpl::open(dir.path(), ColumnarOptions::default()).unwrap();

    engine
        .set_schema(TableSchema {
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    ty: ColumnType::Utf8,
                    nullable: true,
                },
                ColumnDef {
                    name: "count".into(),
                    ty: ColumnType::Int64,
                    nullable: true,
                },
                ColumnDef {
                    name: "score".into(),
                    ty: ColumnType::Float64,
                    nullable: true,
                },
                ColumnDef {
                    name: "active".into(),
                    ty: ColumnType::Bool,
                    nullable: true,
                },
                ColumnDef {
                    name: "payload".into(),
                    ty: ColumnType::Binary,
                    nullable: true,
                },
            ],
        })
        .unwrap();

    engine
        .ingest(vec![
            (
                "id".into(),
                vec![Some(Bytes::from("a")), Some(Bytes::from("b"))],
            ),
            (
                "count".into(),
                vec![Some(Bytes::from("1")), Some(Bytes::from("2"))],
            ),
            (
                "score".into(),
                vec![Some(Bytes::from("3.14")), Some(Bytes::from("2.71"))],
            ),
            (
                "active".into(),
                vec![Some(Bytes::from("true")), Some(Bytes::from("false"))],
            ),
            (
                "payload".into(),
                vec![
                    Some(Bytes::from(&[1u8, 2][..])),
                    Some(Bytes::from(&[3u8, 4][..])),
                ],
            ),
        ])
        .unwrap();

    engine
        .ingest(vec![
            (
                "id".into(),
                vec![Some(Bytes::from("c")), Some(Bytes::from("d"))],
            ),
            (
                "count".into(),
                vec![Some(Bytes::from("3")), Some(Bytes::from("4"))],
            ),
            (
                "score".into(),
                vec![Some(Bytes::from("1.41")), Some(Bytes::from("1.73"))],
            ),
            (
                "active".into(),
                vec![Some(Bytes::from("1")), Some(Bytes::from("0"))],
            ),
            (
                "payload".into(),
                vec![
                    Some(Bytes::from(&[5u8, 6][..])),
                    Some(Bytes::from(&[7u8, 8][..])),
                ],
            ),
        ])
        .unwrap();

    let result = engine
        .scan(
            &["id", "count", "score", "active", "payload"],
            &storage_traits::Predicate::True,
        )
        .unwrap();

    let as_map: std::collections::HashMap<String, Vec<Option<Bytes>>> =
        result.into_iter().collect();
    assert_eq!(
        as_map["id"],
        vec![
            Some(Bytes::from("a")),
            Some(Bytes::from("b")),
            Some(Bytes::from("c")),
            Some(Bytes::from("d"))
        ]
    );
    assert_eq!(
        as_map["count"],
        vec![
            Some(Bytes::from("1")),
            Some(Bytes::from("2")),
            Some(Bytes::from("3")),
            Some(Bytes::from("4"))
        ]
    );
    assert_eq!(
        as_map["score"],
        vec![
            Some(Bytes::from("3.14")),
            Some(Bytes::from("2.71")),
            Some(Bytes::from("1.41")),
            Some(Bytes::from("1.73"))
        ]
    );
    assert_eq!(
        as_map["active"],
        vec![
            Some(Bytes::from("true")),
            Some(Bytes::from("false")),
            Some(Bytes::from("true")),
            Some(Bytes::from("false"))
        ]
    );
    assert_eq!(
        as_map["payload"],
        vec![
            Some(Bytes::from(&[1u8, 2][..])),
            Some(Bytes::from(&[3u8, 4][..])),
            Some(Bytes::from(&[5u8, 6][..])),
            Some(Bytes::from(&[7u8, 8][..])),
        ]
    );
}
