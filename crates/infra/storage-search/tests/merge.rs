use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new().with_field("title", FieldOptions::text())
}

fn open_engine_with_small_segments(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    let mut options = SearchOptions::default_for(schema.clone());
    options.max_segments = 2;
    options.merge_factor = 2;
    options.memtable_size_limit = 64; // force frequent flushes
    SearchEngine::open(dir, options, schema).unwrap()
}

#[test]
fn merge_removes_deletes_and_preserves_data() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine_with_small_segments(dir.path());

    engine
        .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
        .unwrap();
    engine.sync().unwrap();

    engine
        .index_document(b"doc2".to_vec(), Document::new().with_text("title", "hello moon"))
        .unwrap();
    engine.sync().unwrap();

    engine.delete_document(b"doc1").unwrap();
    engine.sync().unwrap();

    // After merge, doc1 should be purged and doc2 should remain.
    let results = engine.search("hello", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc2");

    let doc1 = engine.get_document(b"doc1").unwrap();
    assert!(doc1.is_none());
}
