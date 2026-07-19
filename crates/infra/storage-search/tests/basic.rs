use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn test_schema() -> Schema {
    Schema::new().with_field("title", FieldOptions::text())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = test_schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

#[test]
fn index_get_document() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
        .unwrap();

    let doc = engine.get_document(b"doc1").unwrap();
    assert!(doc.is_some());
    assert_eq!(doc.unwrap().fields.get("title").unwrap().as_text(), Some("hello world"));
}

#[test]
fn delete_document() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
        .unwrap();
    engine.delete_document(b"doc1").unwrap();

    let doc = engine.get_document(b"doc1").unwrap();
    assert!(doc.is_none());

    let results = engine.search("hello", None).unwrap();
    assert!(results.is_empty());
}

#[test]
fn reopen_engine() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = open_engine(dir.path());
        engine
            .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
            .unwrap();
        engine.sync().unwrap();
    }

    let engine = open_engine(dir.path());
    let results = engine.search("hello", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc1");
}

#[test]
fn engine_trait_get_and_scan() {
    use storage_traits::Engine;

    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
        .unwrap();

    let full = engine.get(b"doc1").unwrap().unwrap();
    assert!(!full.is_empty());

    let mut cursor = engine.scan(None, None).unwrap();
    let (key, _value) = cursor.next().unwrap().unwrap();
    assert!(key.as_ref().starts_with(b"doc1"));
}
