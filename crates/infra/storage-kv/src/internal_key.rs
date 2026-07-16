//! Internal key encoding for the LSM engine.
//!
//! An internal key is:
//!
//! ```text
//! | user_key (N bytes) | sequence (7 bytes) | type (1 byte) |
//! ```
//!
//! Sequence numbers increase with newer writes. A snapshot with sequence S
//! sees all entries with sequence <= S; the largest such sequence is the
//! newest visible version.

use bytes::{BufMut, Bytes};

use crate::SequenceNumber;

/// Record type embedded in the low byte of the internal key trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ValueType {
    /// A deletion tombstone.
    Deletion = 0,
    /// A value put.
    Value = 1,
    /// A range-deletion tombstone.
    RangeDeletion = 2,
    /// A reference to a value stored in the blob log.
    BlobRef = 3,
}

impl ValueType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ValueType::Deletion),
            1 => Some(ValueType::Value),
            2 => Some(ValueType::RangeDeletion),
            3 => Some(ValueType::BlobRef),
            _ => None,
        }
    }
}

/// A range tombstone covering `[start, end)` as of `sequence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeTombstone {
    pub start: Vec<u8>,
    pub end: Vec<u8>,
    pub seq: SequenceNumber,
}

impl RangeTombstone {
    /// Encode to a binary form suitable for the MemTable or SSTable meta-block.
    /// Layout: `start_len (LE32) | start | end_len (LE32) | end | seq (BE64)`.
    pub fn encode(&self) -> Vec<u8> {
        use bytes::BufMut;
        let mut buf = Vec::with_capacity(8 + 8 + self.start.len() + self.end.len());
        buf.put_u32_le(self.start.len() as u32);
        buf.put_slice(&self.start);
        buf.put_u32_le(self.end.len() as u32);
        buf.put_slice(&self.end);
        buf.put_u64(self.seq);
        buf
    }

    /// Decode the binary form produced by `encode`.
    pub fn decode(mut buf: &[u8]) -> Option<Self> {
        use bytes::Buf;
        if buf.len() < 8 {
            return None;
        }
        let start_len = buf.get_u32_le() as usize;
        if buf.len() < start_len + 4 {
            return None;
        }
        let start = buf[..start_len].to_vec();
        buf.advance(start_len);
        let end_len = buf.get_u32_le() as usize;
        if buf.len() < end_len + 8 {
            return None;
        }
        let end = buf[..end_len].to_vec();
        buf.advance(end_len);
        let seq = buf.get_u64();
        Some(Self { start, end, seq })
    }

    /// True if `key` is covered by this tombstone (`start <= key < end`).
    pub fn covers(&self, key: &[u8]) -> bool {
        self.start.as_slice() <= key && key < self.end.as_slice()
    }
}

/// Decode the trailer of an internal key.
///
/// The trailer is stored big-endian so that raw byte ordering of internal keys
/// matches the desired comparator order: user key ascending, then sequence
/// descending, then value before deletion for the same sequence.
pub fn parse_internal_key(encoded: &[u8]) -> Option<(SequenceNumber, ValueType)> {
    if encoded.len() < 8 {
        return None;
    }
    let trailer = &encoded[encoded.len() - 8..];
    let num = u64::from_be_bytes([
        trailer[0], trailer[1], trailer[2], trailer[3], trailer[4], trailer[5], trailer[6],
        trailer[7],
    ]);
    let seq = num >> 8;
    let ty = ValueType::from_u8((num & 0xFF) as u8)?;
    Some((seq, ty))
}

/// Extract the user key from an encoded internal key.
pub fn extract_user_key(encoded: &[u8]) -> &[u8] {
    &encoded[..encoded.len().saturating_sub(8)]
}

/// Build an encoded internal key.
///
/// The 8-byte trailer is `(sequence << 8) | type` stored in big-endian byte
/// order. This makes higher sequence numbers compare larger than lower ones for
/// the same user key, and a value record compare larger than a deletion tombstone
/// at the same sequence number.
pub fn build_internal_key(user_key: &[u8], seq: SequenceNumber, ty: ValueType) -> Vec<u8> {
    let mut buf = Vec::with_capacity(user_key.len() + 8);
    buf.put_slice(user_key);
    let num = (seq << 8) | (ty as u64);
    buf.put_u64(num);
    buf
}

/// A parsed internal key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct ParsedInternalKey {
    pub user_key: Bytes,
    pub sequence: SequenceNumber,
    pub ty: ValueType,
}

impl ParsedInternalKey {
    #[allow(dead_code)]
    pub fn parse(encoded: &[u8]) -> Option<Self> {
        let (sequence, ty) = parse_internal_key(encoded)?;
        Some(Self {
            user_key: Bytes::copy_from_slice(extract_user_key(encoded)),
            sequence,
            ty,
        })
    }
}

/// Comparator for encoded internal keys. Orders by user key ascending, then by
/// sequence descending (newer first).
pub fn compare_internal_keys(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let a_user = extract_user_key(a);
    let b_user = extract_user_key(b);
    match a_user.cmp(b_user) {
        std::cmp::Ordering::Equal => {
            let (a_seq, _) = parse_internal_key(a).unwrap_or((0, ValueType::Deletion));
            let (b_seq, _) = parse_internal_key(b).unwrap_or((0, ValueType::Deletion));
            // Sequence numbers increase with newer writes, so the larger
            // sequence (newer) sorts first.
            b_seq.cmp(&a_seq)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = build_internal_key(b"hello", 42, ValueType::Value);
        let parsed = ParsedInternalKey::parse(&key).unwrap();
        assert_eq!(parsed.user_key, &b"hello"[..]);
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.ty, ValueType::Value);
    }

    #[test]
    fn ordering() {
        let a = build_internal_key(b"k", 10, ValueType::Value);
        let b = build_internal_key(b"k", 5, ValueType::Value);
        // Newer (seq 10) should sort before older (seq 5).
        assert_eq!(compare_internal_keys(&a, &b), std::cmp::Ordering::Less);
    }
}
