use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new()
        .with_field("title", FieldOptions::text())
        .with_field("tag", FieldOptions::keyword())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

fn index_docs(engine: &SearchEngine) {
    engine
        .index_document(
            b"doc1".to_vec(),
            Document::new()
                .with_text("title", "hello world")
                .with_text("tag", "alpha"),
        )
        .unwrap();
    engine
        .index_document(
            b"doc2".to_vec(),
            Document::new()
                .with_text("title", "hello moon")
                .with_text("tag", "beta"),
        )
        .unwrap();
    engine
        .index_document(
            b"doc3".to_vec(),
            Document::new()
                .with_text("title", "goodbye world")
                .with_text("tag", "alpha"),
        )
        .unwrap();
}

#[test]
fn term_query() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("hello", None).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn fielded_query() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("tag:alpha", None).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn boolean_and() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("hello AND world", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc1");
}

#[test]
fn boolean_or() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("moon OR goodbye", None).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn boolean_not() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("hello -moon", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc1");
}

#[test]
fn phrase_query() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    engine
        .index_document(
            b"doc1".to_vec(),
            Document::new().with_text("title", "quick brown fox"),
        )
        .unwrap();
    engine
        .index_document(
            b"doc2".to_vec(),
            Document::new().with_text("title", "brown quick fox"),
        )
        .unwrap();

    let results = engine.search("\"quick brown\"", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].doc_id, b"doc1");
}

#[test]
fn prefix_query() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("wor*", None).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn parenthesized_query() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open_engine(dir.path());
    index_docs(&engine);

    let results = engine.search("(hello OR goodbye) AND world", None).unwrap();
    assert_eq!(results.len(), 2);
}
