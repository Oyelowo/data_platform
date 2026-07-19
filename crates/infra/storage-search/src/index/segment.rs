//! Immutable on-disk segment representation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::document::{DocId, Document, decode_document};
use crate::posting::{Posting, decode_postings, encode_postings};
use crate::term::Term;

/// On-disk segment data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentData {
    /// Term dictionary mapping term to postings.
    pub terms: BTreeMap<Term, Vec<u8>>,
    /// Stored documents.
    pub store: BTreeMap<DocId, Vec<u8>>,
    /// Field norms per (doc_id, field).
    pub norms: BTreeMap<(DocId, String), u32>,
    /// Deleted document ids.
    pub deleted: HashSet<DocId>,
}

impl SegmentData {
    /// Create empty segment data.
    pub fn new() -> Self {
        Self {
            terms: BTreeMap::new(),
            store: BTreeMap::new(),
            norms: BTreeMap::new(),
            deleted: HashSet::new(),
        }
    }
}

impl Default for SegmentData {
    fn default() -> Self {
        Self::new()
    }
}

/// An immutable disk segment.
#[derive(Debug, Clone)]
pub struct ImmutableSegment {
    /// Segment directory.
    pub path: PathBuf,
    /// Segment data loaded into memory.
    data: SegmentData,
    /// Total documents originally in the segment.
    pub total_docs: usize,
    /// Average field length per field.
    pub avg_field_lengths: HashMap<String, f32>,
}

impl ImmutableSegment {
    /// Load a segment from a directory.
    pub fn open(dir: impl AsRef<Path>) -> crate::Result<Self> {
        let path = dir.as_ref().to_path_buf();
        let file_path = path.join(super::SEGMENT_FILE);
        let bytes = std::fs::read(&file_path)?;
        let data: SegmentData = bincode::deserialize(&bytes).map_err(crate::Error::corruption)?;
        let total_docs = data.store.len();
        let avg_field_lengths = compute_avg_lengths(&data.norms, total_docs);
        Ok(Self {
            path,
            data,
            total_docs,
            avg_field_lengths,
        })
    }

    /// Create an immutable segment from in-memory data.
    pub fn from_data(dir: impl AsRef<Path>, data: SegmentData) -> crate::Result<Self> {
        let path = dir.as_ref().to_path_buf();
        let total_docs = data.store.len();
        let avg_field_lengths = compute_avg_lengths(&data.norms, total_docs);
        Ok(Self {
            path,
            data,
            total_docs,
            avg_field_lengths,
        })
    }

    /// Return postings for a term, excluding deleted documents.
    pub fn postings(&self, term: &Term) -> crate::Result<Vec<Posting>> {
        match self.data.terms.get(term) {
            Some(bytes) => {
                let postings = decode_postings(bytes)?;
                Ok(postings
                    .into_iter()
                    .filter(|p| !self.data.deleted.contains(&p.doc_id))
                    .collect())
            }
            None => Ok(Vec::new()),
        }
    }

    /// Iterate postings for all terms matching `prefix` in the given field.
    pub fn prefix_postings(
        &self,
        field: Option<&str>,
        prefix: &str,
    ) -> crate::Result<Vec<(Term, Vec<Posting>)>> {
        let mut out = Vec::new();
        for (term, bytes) in self.data.terms.range(..) {
            let field_match = field.is_none_or(|f| term.field == f);
            if !field_match || !term.token.starts_with(prefix) {
                continue;
            }
            let postings: Vec<Posting> = decode_postings(bytes)?
                .into_iter()
                .filter(|p| !self.data.deleted.contains(&p.doc_id))
                .collect();
            if !postings.is_empty() {
                out.push((term.clone(), postings));
            }
        }
        Ok(out)
    }

    /// Return all terms starting with `prefix`.
    pub fn terms_prefix(&self, prefix: &str) -> impl Iterator<Item = &Term> {
        self.data
            .terms
            .range(..)
            .map(|(t, _)| t)
            .filter(move |t| t.token.starts_with(prefix))
    }

    /// Return stored document bytes.
    pub fn stored(&self, doc_id: &DocId) -> Option<&[u8]> {
        if self.data.deleted.contains(doc_id) {
            return None;
        }
        self.data.store.get(doc_id).map(|v| v.as_slice())
    }

    /// Return stored document decoded.
    pub fn get_document(&self, doc_id: &DocId) -> Option<Document> {
        let bytes = self.stored(doc_id)?;
        decode_document(bytes).ok()
    }

    /// Return field norm.
    pub fn norm(&self, doc_id: &DocId, field: &str) -> u32 {
        self.data
            .norms
            .get(&(doc_id.clone(), field.to_string()))
            .copied()
            .unwrap_or(1)
    }

    /// Number of documents in the segment (including deleted).
    pub fn len(&self) -> usize {
        self.total_docs
    }

    /// Return true if the segment contains no documents.
    pub fn is_empty(&self) -> bool {
        self.total_docs == 0
    }

    /// Number of live documents.
    pub fn live_docs(&self) -> usize {
        self.total_docs - self.data.deleted.len()
    }

    /// Mark a document as deleted.
    pub fn delete_document(&mut self, doc_id: &DocId) {
        self.data.deleted.insert(doc_id.clone());
    }

    /// Return the number of documents containing a term (document frequency).
    pub fn doc_freq(&self, term: &Term) -> usize {
        match self.data.terms.get(term) {
            Some(bytes) => match decode_postings(bytes) {
                Ok(postings) => postings
                    .into_iter()
                    .filter(|p| !self.data.deleted.contains(&p.doc_id))
                    .count(),
                Err(_) => 0,
            },
            None => 0,
        }
    }

    /// Return average field length.
    pub fn avg_field_length(&self, field: &str) -> f32 {
        self.avg_field_lengths.get(field).copied().unwrap_or(1.0)
    }

    /// Return all live doc ids.
    pub fn live_doc_ids(&self) -> Vec<DocId> {
        self.data
            .store
            .keys()
            .filter(|id| !self.data.deleted.contains(*id))
            .cloned()
            .collect()
    }

    /// Return the underlying segment data for merging.
    pub fn into_data(self) -> SegmentData {
        self.data
    }
}

fn compute_avg_lengths(norms: &BTreeMap<(DocId, String), u32>, _total_docs: usize) -> HashMap<String, f32> {
    let mut sums: HashMap<String, u64> = HashMap::new();
    let mut counts: HashMap<String, u32> = HashMap::new();
    for ((_, field), norm) in norms {
        *sums.entry(field.clone()).or_default() += u64::from(*norm);
        *counts.entry(field.clone()).or_default() += 1;
    }
    sums.into_iter()
        .map(|(field, sum)| {
            let count = counts.get(&field).copied().unwrap_or(1).max(1);
            let avg = sum as f32 / count as f32;
            (field, avg.max(1.0))
        })
        .collect()
}

/// Encode a term dictionary value from postings.
pub fn encode_term_postings(postings: &[Posting]) -> Vec<u8> {
    encode_postings(postings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_postings() {
        let mut data = SegmentData::new();
        data.terms.insert(
            Term::new("title", "hello"),
            encode_term_postings(&[Posting::new(b"doc1".to_vec(), 1, vec![])]),
        );
        data.store.insert(b"doc1".to_vec(), b"{}".to_vec());
        let seg = ImmutableSegment::from_data("/tmp", data).unwrap();
        let postings = seg.postings(&Term::new("title", "hello")).unwrap();
        assert_eq!(postings.len(), 1);
    }
}
