//! Merge immutable segments and purge deleted documents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::document::DocId;
use crate::index::segment::{SegmentData, encode_term_postings};
use crate::posting::{Posting, decode_postings};
use crate::term::Term;

/// Merge multiple segments into a single segment directory.
pub fn merge_segments(
    dir: impl AsRef<Path>,
    segment_id: u64,
    segments: &[crate::index::segment::ImmutableSegment],
) -> crate::Result<PathBuf> {
    let merged = merge_data(segments)?;
    write_merged_segment(dir, segment_id, &merged)
}

fn merge_data(segments: &[crate::index::segment::ImmutableSegment]) -> crate::Result<SegmentData> {
    let mut data = SegmentData::new();

    // Merge posting lists by term and doc_id.
    let mut term_postings: BTreeMap<Term, BTreeMap<DocId, (u32, Vec<u32>)>> = BTreeMap::new();
    for seg in segments {
        let seg_data = seg.clone().into_data();
        for (term, bytes) in seg_data.terms {
            let postings = decode_postings(&bytes)?;
            let entry = term_postings.entry(term).or_default();
            for p in postings {
                if seg_data.deleted.contains(&p.doc_id) {
                    continue;
                }
                let doc_entry = entry.entry(p.doc_id).or_insert((0, Vec::new()));
                doc_entry.0 += p.term_freq;
                doc_entry.1.extend(p.positions);
            }
        }
    }

    for (term, docs) in term_postings {
        let postings: Vec<Posting> = docs
            .into_iter()
            .map(|(doc_id, (term_freq, positions))| Posting::new(doc_id, term_freq, positions))
            .collect();
        data.terms.insert(term, encode_term_postings(&postings));
    }

    // Merge stored documents.
    for seg in segments {
        let seg_data = seg.clone().into_data();
        for (doc_id, stored) in seg_data.store {
            if !seg_data.deleted.contains(&doc_id) {
                data.store.insert(doc_id, stored);
            }
        }
    }

    // Merge norms.
    for seg in segments {
        let seg_data = seg.clone().into_data();
        for ((doc_id, field), norm) in seg_data.norms {
            if !seg_data.deleted.contains(&doc_id) {
                data.norms.insert((doc_id, field), norm);
            }
        }
    }

    Ok(data)
}

fn write_merged_segment(
    dir: impl AsRef<Path>,
    segment_id: u64,
    data: &SegmentData,
) -> crate::Result<PathBuf> {
    let base = dir.as_ref();
    let segment_dir = base.join(format!("segment_{segment_id:016x}"));
    let tmp_dir = base.join(format!("segment_{segment_id:016x}.tmp"));

    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    let encoded = bincode::serialize(data).map_err(crate::Error::corruption)?;
    storage_file::atomic_write(&tmp_dir.join(super::SEGMENT_FILE), &encoded)?;

    if segment_dir.exists() {
        std::fs::remove_dir_all(&segment_dir)?;
    }
    std::fs::rename(&tmp_dir, &segment_dir)?;

    Ok(segment_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::index::memory::MemorySegment;
    use crate::index::segment::ImmutableSegment;
    use crate::index::writer::SegmentWriter;
    use crate::schema::{FieldOptions, Schema};

    #[test]
    fn merge_two_segments() {
        let schema = Schema::new().with_field("title", FieldOptions::text());
        let dir = tempfile::tempdir().unwrap();

        let mut mem1 = MemorySegment::new();
        mem1.index_document(b"doc1".to_vec(), &Document::new().with_text("title", "hello world"), &schema).unwrap();
        let path1 = SegmentWriter::write(dir.path(), 1, &mem1).unwrap();

        let mut mem2 = MemorySegment::new();
        mem2.index_document(b"doc2".to_vec(), &Document::new().with_text("title", "hello moon"), &schema).unwrap();
        let path2 = SegmentWriter::write(dir.path(), 2, &mem2).unwrap();

        let seg1 = ImmutableSegment::open(&path1).unwrap();
        let seg2 = ImmutableSegment::open(&path2).unwrap();

        let merged_dir = tempfile::tempdir().unwrap();
        let merged_path = merge_segments(merged_dir.path(), 3, &[seg1, seg2]).unwrap();
        let merged = ImmutableSegment::open(&merged_path).unwrap();

        let postings = merged.postings(&crate::term::Term::new("title", "hello")).unwrap();
        assert_eq!(postings.len(), 2);
    }
}
