//! BM25 scoring.

use std::collections::HashSet;

use crate::document::{DocId, Document};
use crate::posting::Posting;
use crate::term::Term;

/// Segment-level information needed for scoring.
pub trait SegmentSource {
    /// Error type.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Postings for a term.
    fn postings(&self, term: &Term) -> Result<Vec<Posting>, Self::Error>;

    /// All postings matching a prefix.
    fn prefix_postings(
        &self,
        field: Option<&str>,
        prefix: &str,
    ) -> Result<Vec<(Term, Vec<Posting>)>, Self::Error>;

    /// Average field length.
    fn avg_field_length(&self, field: &str) -> f32;

    /// Total number of documents in the segment.
    fn total_docs(&self) -> usize;

    /// Number of documents containing a term.
    fn doc_freq(&self, term: &Term) -> usize;

    /// Field norm for a document.
    fn norm(&self, doc_id: &DocId, field: &str) -> u32;

    /// Return all live document ids in the segment.
    fn all_doc_ids(&self) -> Vec<DocId>;

    /// Return stored document, if available.
    fn stored(&self, doc_id: &DocId) -> Option<Document>;
}

/// BM25 scorer.
#[derive(Debug, Clone, Copy)]
pub struct Bm25Scorer {
    k1: f32,
    b: f32,
}

impl Bm25Scorer {
    /// Create a scorer with the given BM25 parameters.
    pub fn new(k1: f32, b: f32) -> Self {
        Self { k1, b }
    }

    /// Compute IDF for a term.
    pub fn idf(&self, total_docs: usize, doc_freq: usize) -> f32 {
        let n = total_docs as f32;
        let df = doc_freq as f32;
        ((n - df + 0.5) / (df + 0.5)).ln_1p()
    }

    /// Compute BM25 score for one term in one document.
    pub fn score(
        &self,
        tf: u32,
        field_len: u32,
        avg_field_len: f32,
        idf: f32,
    ) -> f32 {
        let tf = tf as f32;
        let field_len = field_len as f32;
        let denom = tf + self.k1 * (1.0 - self.b + self.b * field_len / avg_field_len.max(1.0));
        idf * (tf * (self.k1 + 1.0) / denom)
    }

    /// Score a posting against a term.
    pub fn score_posting(
        &self,
        source: &dyn SegmentSource<Error = crate::Error>,
        term: &Term,
        posting: &Posting,
    ) -> f32 {
        let idf = self.idf(source.total_docs(), source.doc_freq(term));
        let field_len = source.norm(&posting.doc_id, &term.field);
        let avg = source.avg_field_length(&term.field);
        self.score(posting.term_freq, field_len, avg, idf)
    }
}

impl Default for Bm25Scorer {
    fn default() -> Self {
        Self { k1: 1.2, b: 0.75 }
    }
}

/// Score a document against a list of postings grouped by term.
pub fn score_document(
    source: &dyn SegmentSource<Error = crate::Error>,
    scorer: &Bm25Scorer,
    terms: &[Term],
) -> Result<Vec<(DocId, f32)>, crate::Error> {
    let mut scores: std::collections::HashMap<DocId, f32> = std::collections::HashMap::new();
    for term in terms {
        let postings = source.postings(term).map_err(|e| crate::Error::Corruption(e.to_string()))?;
        for posting in postings {
            let s = scorer.score_posting(source, term, &posting);
            *scores.entry(posting.doc_id).or_default() += s;
        }
    }
    Ok(scores.into_iter().collect())
}

/// Intersect two posting lists by doc_id, returning the left postings for docs
/// present in both.
pub fn intersect_postings(left: &[Posting], right: &[Posting]) -> Vec<Posting> {
    let right_ids: HashSet<&DocId> = right.iter().map(|p| &p.doc_id).collect();
    left.iter()
        .filter(|p| right_ids.contains(&p.doc_id))
        .cloned()
        .collect()
}

/// Union two posting lists by doc_id.
pub fn union_postings(left: &[Posting], right: &[Posting]) -> Vec<Posting> {
    let mut map: std::collections::BTreeMap<DocId, Posting> = std::collections::BTreeMap::new();
    for p in left.iter().chain(right.iter()) {
        map.entry(p.doc_id.clone()).or_insert_with(|| Posting::new(p.doc_id.clone(), 0, Vec::new()));
    }
    map.into_values().collect()
}

/// Check whether positions form a consecutive phrase.
pub fn positions_match_phrase(position_lists: &[&[u32]], slop: u32) -> bool {
    if position_lists.is_empty() {
        return false;
    }
    if position_lists.len() == 1 {
        return !position_lists[0].is_empty();
    }
    let first = position_lists[0];
    for &start in first {
        if positions_match_from(position_lists, 1, start, slop) {
            return true;
        }
    }
    false
}

fn positions_match_from(lists: &[&[u32]], idx: usize, expected: u32, slop: u32) -> bool {
    if idx >= lists.len() {
        return true;
    }
    let min = expected + 1;
    let max = expected + 1 + slop;
    for &pos in lists[idx] {
        if (min..=max).contains(&pos) && positions_match_from(lists, idx + 1, pos, slop) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idf_is_positive() {
        let scorer = Bm25Scorer::default();
        let idf = scorer.idf(100, 10);
        assert!(idf > 0.0);
    }

    #[test]
    fn phrase_positions_match() {
        let lists: Vec<&[u32]> = vec![&[0, 5], &[1, 6]];
        assert!(positions_match_phrase(&lists, 0));
        let lists2: Vec<&[u32]> = vec![&[0], &[2]];
        assert!(!positions_match_phrase(&lists2, 0));
    }
}
