//! In-memory mutable search segment.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::analyzer::analyze;
use crate::document::{DocId, Document, decode_document, encode_document};
use crate::posting::Posting;
use crate::schema::{FieldOptions, FieldValue, Schema};
use crate::term::Term;

/// A mutable in-memory segment.
#[derive(Debug, Clone, Default)]
pub struct MemorySegment {
    /// Inverted index: term -> sorted posting list.
    terms: BTreeMap<Term, Vec<Posting>>,
    /// Stored field values per document.
    store: BTreeMap<DocId, Vec<u8>>,
    /// Field norms per (doc_id, field).
    norms: BTreeMap<(DocId, String), u32>,
    /// Deleted document ids.
    deleted: HashSet<DocId>,
}

impl MemorySegment {
    /// Create a new empty memory segment.
    pub fn new() -> Self {
        Self {
            terms: BTreeMap::new(),
            store: BTreeMap::new(),
            norms: BTreeMap::new(),
            deleted: HashSet::new(),
        }
    }

    /// Index a document.
    pub fn index_document(
        &mut self,
        doc_id: DocId,
        document: &Document,
        schema: &Schema,
    ) -> crate::Result<()> {
        document.validate(schema)?;

        // Remove previous entries for the same doc id (upsert semantics).
        self.delete_document(&doc_id);
        self.deleted.remove(&doc_id);

        let mut stored = false;
        for (field_name, value) in &document.fields {
            let options = schema.get(field_name).ok_or_else(|| {
                crate::Error::invalid_argument(format!("unknown field: {field_name}"))
            })?;

            if options.stored {
                stored = true;
            }

            if !options.indexed {
                continue;
            }

            let text: String = match value {
                FieldValue::Text(t) => t.clone(),
                FieldValue::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            };

            index_text(self, &doc_id, field_name, &text, options);
        }

        if stored {
            let encoded = encode_document(document)?;
            self.store.insert(doc_id.clone(), encoded);
        }

        Ok(())
    }

    /// Mark a document as deleted.
    pub fn delete_document(&mut self, doc_id: &DocId) {
        if !self.deleted.insert(doc_id.clone()) {
            return;
        }

        // Remove stored document and postings.
        self.store.remove(doc_id);
        self.norms.retain(|(id, _), _| id != doc_id);

        let mut empty_terms = Vec::new();
        for (term, postings) in &mut self.terms {
            postings.retain(|p| &p.doc_id != doc_id);
            if postings.is_empty() {
                empty_terms.push(term.clone());
            }
        }
        for term in empty_terms {
            self.terms.remove(&term);
        }
    }

    /// Return postings for a term, excluding deleted documents.
    pub fn postings(&self, term: &Term) -> &[Posting] {
        self.terms.get(term).map_or(&[], |v| v.as_slice())
    }

    /// Return stored document bytes for a doc id.
    pub fn stored(&self, doc_id: &DocId) -> Option<&[u8]> {
        if self.deleted.contains(doc_id) {
            return None;
        }
        self.store.get(doc_id).map(|v| v.as_slice())
    }

    /// Return the stored document decoded.
    pub fn get_document(&self, doc_id: &DocId) -> Option<Document> {
        let bytes = self.stored(doc_id)?;
        decode_document(bytes).ok()
    }

    /// Return field norm for a document/field.
    pub fn norm(&self, doc_id: &DocId, field: &str) -> u32 {
        self.norms
            .get(&(doc_id.clone(), field.to_string()))
            .copied()
            .unwrap_or(1)
    }

    /// Return the number of indexed documents.
    pub fn doc_count(&self) -> usize {
        self.store.len()
    }

    /// Return true if the segment contains no documents.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Approximate memory footprint in bytes.
    pub fn approx_bytes(&self) -> usize {
        let mut total = 0usize;
        for (term, postings) in &self.terms {
            total += term.field.len() + term.token.len();
            for p in postings {
                total += p.doc_id.len() + 12 + p.positions.len() * 4;
            }
        }
        for (k, v) in &self.store {
            total += k.len() + v.len();
        }
        total += self.norms.len() * 32;
        total
    }

    /// Iterate over all terms.
    pub fn terms(&self) -> &BTreeMap<Term, Vec<Posting>> {
        &self.terms
    }

    /// Iterate over all stored documents.
    pub fn stored_docs(&self) -> impl Iterator<Item = (&DocId, &Vec<u8>)> {
        self.store.iter()
    }

    /// Return deleted document ids.
    pub fn deleted(&self) -> &HashSet<DocId> {
        &self.deleted
    }

    /// Return norms map.
    pub fn norms(&self) -> &BTreeMap<(DocId, String), u32> {
        &self.norms
    }
}

fn index_text(
    segment: &mut MemorySegment,
    doc_id: &DocId,
    field_name: &str,
    text: &str,
    options: &FieldOptions,
) {
    let tokens = analyze(text, options);
    let length = tokens.len() as u32;
    segment
        .norms
        .insert((doc_id.clone(), field_name.to_string()), length.max(1));

    let mut positions_by_token: HashMap<String, Vec<u32>> = HashMap::new();
    for token in tokens {
        positions_by_token
            .entry(token.text)
            .or_default()
            .push(token.position);
    }

    for (token, positions) in positions_by_token {
        let term = Term::new(field_name.to_string(), token);
        let term_freq = positions.len() as u32;
        let stored_positions = if options.with_positions { positions } else { Vec::new() };
        let posting = Posting::new(doc_id.clone(), term_freq, stored_positions);
        segment.terms.entry(term).or_default().push(posting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FieldOptions, Schema};

    fn schema() -> Schema {
        Schema::new()
            .with_field("title", FieldOptions::text())
            .with_field("id", FieldOptions::keyword())
    }

    #[test]
    fn index_and_lookup() {
        let mut seg = MemorySegment::new();
        let doc = Document::new().with_text("title", "hello world");
        seg.index_document(b"doc1".to_vec(), &doc, &schema()).unwrap();
        assert_eq!(seg.postings(&Term::new("title", "hello")).len(), 1);
        assert_eq!(seg.doc_count(), 1);
    }

    #[test]
    fn delete_removes_postings() {
        let mut seg = MemorySegment::new();
        let doc = Document::new().with_text("title", "hello world");
        seg.index_document(b"doc1".to_vec(), &doc, &schema()).unwrap();
        seg.delete_document(&b"doc1".to_vec());
        assert!(seg.postings(&Term::new("title", "hello")).is_empty());
        assert!(seg.is_empty());
    }
}
