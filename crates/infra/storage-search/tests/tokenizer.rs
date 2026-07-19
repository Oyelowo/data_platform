use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new().with_field("body", FieldOptions::text())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

#[test]
fn unicode_tokenization() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(
            b"doc1".to_vec(),
            Document::new().with_text("body", "Café résumé naïve"),
        )
        .unwrap();

    let results = engine.search("café", None).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn stemming_matches_variant() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(
            b"doc1".to_vec(),
            Document::new().with_text("body", "running quickly"),
        )
        .unwrap();

    // "run" should match the stemmed "running".
    let results = engine.search("run", None).unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn stop_words_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(
            b"doc1".to_vec(),
            Document::new().with_text("body", "the quick brown fox"),
        )
        .unwrap();

    let results = engine.search("the", None).unwrap();
    assert!(results.is_empty());
}
