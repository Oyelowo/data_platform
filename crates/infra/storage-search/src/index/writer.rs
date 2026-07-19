//! Serialize a memory segment to an immutable disk segment.

use std::path::{Path, PathBuf};

use crate::index::memory::MemorySegment;
use crate::index::segment::{SegmentData, encode_term_postings};
use crate::posting::Posting;

/// Segment writer.
pub struct SegmentWriter;

impl SegmentWriter {
    /// Write a memory segment to `dir` atomically.
    pub fn write(
        dir: impl AsRef<Path>,
        segment_id: u64,
        memory: &MemorySegment,
    ) -> crate::Result<PathBuf> {
        let base = dir.as_ref();
        let segment_dir = base.join(format!("segment_{segment_id:016x}"));
        std::fs::create_dir_all(&segment_dir)?;

        let tmp_dir = base.join(format!("segment_{segment_id:016x}.tmp"));
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir)?;
        }
        std::fs::create_dir_all(&tmp_dir)?;

        let data = build_segment_data(memory)?;
        let encoded = bincode::serialize(&data).map_err(crate::Error::corruption)?;
        storage_file::atomic_write(&tmp_dir.join(super::SEGMENT_FILE), &encoded)?;

        if segment_dir.exists() {
            std::fs::remove_dir_all(&segment_dir)?;
        }
        std::fs::rename(&tmp_dir, &segment_dir)?;

        Ok(segment_dir)
    }
}

fn build_segment_data(memory: &MemorySegment) -> crate::Result<SegmentData> {
    let mut data = SegmentData::new();

    for (term, postings) in memory.terms() {
        let live: Vec<Posting> = postings
            .iter()
            .filter(|p| !memory.deleted().contains(&p.doc_id))
            .cloned()
            .collect();
        if !live.is_empty() {
            data.terms.insert(term.clone(), encode_term_postings(&live));
        }
    }

    for (doc_id, encoded) in memory.stored_docs() {
        if !memory.deleted().contains(doc_id) {
            data.store.insert(doc_id.clone(), encoded.clone());
        }
    }

    for ((doc_id, field), norm) in memory.norms() {
        if !memory.deleted().contains(doc_id) {
            data.norms
                .insert((doc_id.clone(), field.clone()), *norm);
        }
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::schema::{FieldOptions, Schema};
    use crate::term::Term;

    #[test]
    fn write_and_read_segment() {
        let mut memory = MemorySegment::new();
        let schema = Schema::new().with_field("title", FieldOptions::text());
        let doc = Document::new().with_text("title", "hello world");
        memory.index_document(b"doc1".to_vec(), &doc, &schema).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = SegmentWriter::write(dir.path(), 1, &memory).unwrap();
        assert!(path.exists());

        let seg = super::super::segment::ImmutableSegment::open(&path).unwrap();
        let postings = seg.postings(&Term::new("title", "hello")).unwrap();
        assert_eq!(postings.len(), 1);
    }
}
