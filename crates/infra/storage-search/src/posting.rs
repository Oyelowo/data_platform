//! Posting list representation and serialization.

use std::collections::BTreeMap;

use bytes::{Buf, BufMut};

use crate::document::DocId;

/// A single posting entry for a term in one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posting {
    /// Document identifier.
    pub doc_id: DocId,
    /// Term frequency in this document.
    pub term_freq: u32,
    /// Term positions in this document, if requested.
    pub positions: Vec<u32>,
}

impl Posting {
    /// Create a new posting.
    pub fn new(doc_id: DocId, term_freq: u32, positions: Vec<u32>) -> Self {
        Self {
            doc_id,
            term_freq,
            positions,
        }
    }
}

/// Encode a posting list to bytes.
///
/// Format:
/// - count (u32 LE)
/// - for each posting:
///   - doc_id_len (u32 LE)
///   - doc_id bytes
///   - term_freq (u32 LE)
///   - positions_count (u32 LE)
///   - positions (u32 LE each)
pub fn encode_postings(postings: &[Posting]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.put_u32_le(postings.len() as u32);
    for p in postings {
        buf.put_u32_le(p.doc_id.len() as u32);
        buf.extend_from_slice(&p.doc_id);
        buf.put_u32_le(p.term_freq);
        buf.put_u32_le(p.positions.len() as u32);
        for &pos in &p.positions {
            buf.put_u32_le(pos);
        }
    }
    buf
}

/// Decode a posting list from bytes.
pub fn decode_postings(buf: &[u8]) -> crate::Result<Vec<Posting>> {
    if buf.len() < 4 {
        return Err(crate::Error::corruption("postings buffer too short"));
    }
    let mut cursor = buf;
    let count = cursor.get_u32_le() as usize;
    let mut postings = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor.len() < 4 {
            return Err(crate::Error::corruption("truncated doc_id length"));
        }
        let doc_id_len = cursor.get_u32_le() as usize;
        if cursor.len() < doc_id_len + 8 {
            return Err(crate::Error::corruption("truncated posting"));
        }
        let doc_id = cursor.copy_to_bytes(doc_id_len).to_vec();
        let term_freq = cursor.get_u32_le();
        if cursor.len() < 4 {
            return Err(crate::Error::corruption("truncated positions count"));
        }
        let positions_count = cursor.get_u32_le() as usize;
        if cursor.len() < positions_count * 4 {
            return Err(crate::Error::corruption("truncated positions"));
        }
        let mut positions = Vec::with_capacity(positions_count);
        for _ in 0..positions_count {
            positions.push(cursor.get_u32_le());
        }
        postings.push(Posting {
            doc_id,
            term_freq,
            positions,
        });
    }
    Ok(postings)
}

/// Merge multiple sorted posting lists into one, summing term frequencies and
/// concatenating positions when the same doc_id appears in multiple lists.
pub fn merge_posting_lists(
    lists: Vec<Vec<Posting>>,
) -> BTreeMap<DocId, (u32, Vec<u32>)> {
    let mut merged: BTreeMap<DocId, (u32, Vec<u32>)> = BTreeMap::new();
    for list in lists {
        for p in list {
            let entry = merged.entry(p.doc_id).or_insert((0, Vec::new()));
            entry.0 += p.term_freq;
            entry.1.extend(p.positions);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postings_roundtrip() {
        let postings = vec![
            Posting::new(b"doc1".to_vec(), 2, vec![0, 5]),
            Posting::new(b"doc2".to_vec(), 1, vec![3]),
        ];
        let encoded = encode_postings(&postings);
        let decoded = decode_postings(&encoded).unwrap();
        assert_eq!(postings, decoded);
    }

    #[test]
    fn merge_postings() {
        let a = vec![Posting::new(b"doc1".to_vec(), 2, vec![1, 3])];
        let b = vec![Posting::new(b"doc1".to_vec(), 1, vec![5])];
        let merged = merge_posting_lists(vec![a, b]);
        assert_eq!(merged.get(b"doc1".as_slice()), Some(&(3, vec![1, 3, 5])));
    }
}
