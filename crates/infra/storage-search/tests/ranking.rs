use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new().with_field("body", FieldOptions::text())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

#[test]
fn bm25_ranks_more_relevant_higher() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());

    // Short, focused document should outrank a long document with one mention.
    engine
        .index_document(
            b"relevant".to_vec(),
            Document::new().with_text("body", "rust rust rust"),
        )
        .unwrap();
    engine
        .index_document(
            b"long".to_vec(),
            Document::new().with_text(
                "body",
                "rust is a programming language with many features and a large ecosystem",
            ),
        )
        .unwrap();

    let results = engine.search("rust", Some(10)).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].doc_id, b"relevant");
    assert!(results[0].score > results[1].score);
}
