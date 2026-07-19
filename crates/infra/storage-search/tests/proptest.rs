use proptest::prelude::*;
use storage_search::{Document, FieldOptions, Schema, SearchEngine, SearchOptions};

fn schema() -> Schema {
    Schema::new().with_field("body", FieldOptions::text())
}

fn open_engine(dir: &std::path::Path) -> SearchEngine {
    let schema = schema();
    SearchEngine::open(dir, SearchOptions::default_for(schema.clone()), schema).unwrap()
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|s| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|s| !s.is_empty() && !storage_search::tokenizer::is_stop_word(s))
        .collect()
}

fn matches_term(doc: &Document, term: &str) -> bool {
    doc.fields
        .values()
        .filter_map(|v| v.as_text())
        .any(|text| tokenize(text).contains(&term.to_lowercase()))
}

proptest! {
    #[test]
    fn random_docs_term_query(docs in prop::collection::vec(
        ("doc[0-9]{1,3}", "[a-z ]{0,40}"),
        1..30
    )) {
        let dir = tempfile::tempdir().unwrap();
        let engine = open_engine(dir.path());

        let mut indexed: std::collections::HashMap<String, Document> = std::collections::HashMap::new();
        for (id, text) in &docs {
            let doc = Document::new().with_text("body", text.clone());
            engine.index_document(id.clone().into_bytes(), doc.clone()).unwrap();
            indexed.insert(id.clone(), doc);
        }
        engine.sync().unwrap();

        // Pick a random term from one of the documents and verify the result set.
        if let Some((_, query_doc)) = indexed.iter().next() {
            let tokens = tokenize(query_doc.fields.get("body").unwrap().as_text().unwrap_or(""));
            if let Some(term) = tokens.first() {
                let results = engine.search(term, Some(100)).unwrap();
                let result_ids: std::collections::HashSet<String> = results
                    .into_iter()
                    .map(|r| String::from_utf8(r.doc_id).unwrap())
                    .collect();

                let expected_ids: std::collections::HashSet<String> = indexed
                    .iter()
                    .filter(|(_, doc)| matches_term(doc, term))
                    .map(|(id, _)| id.clone())
                    .collect();

                prop_assert_eq!(result_ids, expected_ids);
            }
        }
    }
}
