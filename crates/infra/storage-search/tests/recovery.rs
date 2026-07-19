use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new().with_field("title", FieldOptions::text())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

#[test]
fn wal_replay_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = open_engine(dir.path());
        engine
            .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
            .unwrap();
        // Do not sync; close without flushing.
    }

    let engine = open_engine(dir.path());
    let results = engine.search("hello", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc1");
}

#[test]
fn wal_replay_delete_survives() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = open_engine(dir.path());
        engine
            .index_document(b"doc1".to_vec(), Document::new().with_text("title", "hello world"))
            .unwrap();
        engine.delete_document(b"doc1").unwrap();
    }

    let engine = open_engine(dir.path());
    let results = engine.search("hello", None).unwrap();
    assert!(results.is_empty());
}
