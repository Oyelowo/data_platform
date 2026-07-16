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
}

impl ValueType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ValueType::Deletion),
            1 => Some(ValueType::Value),
            _ => None,
        }
    }
}

/// Decode the trailer of an internal key.
pub fn parse_internal_key(encoded: &[u8]) -> Option<(SequenceNumber, ValueType)> {
    if encoded.len() < 8 {
        return None;
    }
    let trailer = &encoded[encoded.len() - 8..];
    let num = u64::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3], trailer[4], trailer[5], trailer[6], trailer[7]]);
    let seq = num >> 8;
    let ty = ValueType::from_u8((num & 0xFF) as u8)?;
    Some((seq, ty))
}

/// Extract the user key from an encoded internal key.
pub fn extract_user_key(encoded: &[u8]) -> &[u8] {
    &encoded[..encoded.len().saturating_sub(8)]
}

/// Build an encoded internal key.
pub fn build_internal_key(user_key: &[u8], seq: SequenceNumber, ty: ValueType) -> Vec<u8> {
    let mut buf = Vec::with_capacity(user_key.len() + 8);
    buf.put_slice(user_key);
    let num = (seq << 8) | (ty as u64);
    buf.put_u64_le(num);
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
