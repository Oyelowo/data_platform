//! Read access to an immutable segment.

use crate::document::{DocId, Document};
use crate::index::segment::ImmutableSegment;
use crate::posting::Posting;
use crate::term::Term;

/// Reader over an immutable segment.
#[derive(Debug, Clone)]
pub struct SegmentReader {
    segment: ImmutableSegment,
}

impl SegmentReader {
    /// Open a reader over a segment directory.
    pub fn open(dir: impl AsRef<std::path::Path>) -> crate::Result<Self> {
        Ok(Self {
            segment: ImmutableSegment::open(dir)?,
        })
    }

    /// Return postings for a term.
    pub fn postings(&self, term: &Term) -> crate::Result<Vec<Posting>> {
        self.segment.postings(term)
    }

    /// Return postings for all terms matching a prefix.
    pub fn prefix_postings(
        &self,
        field: Option<&str>,
        prefix: &str,
    ) -> crate::Result<Vec<(Term, Vec<Posting>)>> {
        self.segment.prefix_postings(field, prefix)
    }

    /// Return stored document bytes.
    pub fn stored(&self, doc_id: &DocId) -> Option<&[u8]> {
        self.segment.stored(doc_id)
    }

    /// Return stored document decoded.
    pub fn get_document(&self, doc_id: &DocId) -> Option<Document> {
        self.segment.get_document(doc_id)
    }

    /// Return field norm.
    pub fn norm(&self, doc_id: &DocId, field: &str) -> u32 {
        self.segment.norm(doc_id, field)
    }

    /// Average field length.
    pub fn avg_field_length(&self, field: &str) -> f32 {
        self.segment.avg_field_length(field)
    }

    /// Total documents in the segment.
    pub fn len(&self) -> usize {
        self.segment.len()
    }

    /// Return true if the segment contains no documents.
    pub fn is_empty(&self) -> bool {
        self.segment.is_empty()
    }

    /// Live documents in the segment.
    pub fn live_docs(&self) -> usize {
        self.segment.live_docs()
    }

    /// Document frequency for a term.
    pub fn doc_freq(&self, term: &Term) -> usize {
        self.segment.doc_freq(term)
    }

    /// Inner segment.
    pub fn segment(&self) -> &ImmutableSegment {
        &self.segment
    }
}
