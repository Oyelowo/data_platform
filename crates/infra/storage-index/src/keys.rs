//! Internal key prefix encoding.
//!
//! Keys in the underlying engine are prefixed so that primary records and each
//! secondary index occupy disjoint, ordered ranges.

use std::mem::size_of;

/// Type tag for primary records.
pub(crate) const TAG_PRIMARY: u8 = 0x01;

/// Type tag for index entries.
pub(crate) const TAG_INDEX: u8 = 0x02;

const INDEX_ID_BYTES: usize = size_of::<u32>();

/// Escape marker used in index keys. A raw `0x00` byte is encoded as
/// `[ESCAPE, 0x01]` and the terminator is `[ESCAPE, ESCAPE]`. This makes the
/// encoded byte string order-preserving: lexicographic comparison of escaped
/// strings matches comparison of the original strings.
const ESCAPE: u8 = 0x00;
const ESCAPED_ZERO: u8 = 0x01;
const TERMINATOR: [u8; 2] = [ESCAPE, ESCAPE];

/// Build the internal key for a primary record.
pub fn primary_key(user_key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + user_key.len());
    out.push(TAG_PRIMARY);
    out.extend_from_slice(user_key);
    out
}

/// Extract the user key from an internal primary key, returning `None` if the
/// tag is wrong.
pub fn unpack_primary_key(internal: &[u8]) -> Option<&[u8]> {
    if internal.first()? != &TAG_PRIMARY {
        return None;
    }
    Some(&internal[1..])
}

/// Encode a column value for use inside an index key. The encoding is
/// order-preserving and self-terminating.
fn escape_column(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() + 2);
    for &b in value {
        if b == ESCAPE {
            out.push(ESCAPE);
            out.push(ESCAPED_ZERO);
        } else {
            out.push(b);
        }
    }
    out.extend_from_slice(&TERMINATOR);
    out
}

/// Decode an escaped column value. Returns the consumed length (including the
/// terminator) alongside the decoded bytes.
fn unescape_column(bytes: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == ESCAPE {
            if i + 1 >= bytes.len() {
                return None;
            }
            match bytes[i + 1] {
                ESCAPED_ZERO => {
                    out.push(ESCAPE);
                    i += 2;
                }
                ESCAPE => {
                    // End of column value. Return decoded bytes and consumed
                    // length (including the terminator).
                    let consumed = i + 2;
                    return Some((out, consumed));
                }
                _ => return None,
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    None
}

/// Build the internal key for an index entry.
///
/// The key layout is: `TAG_INDEX | index_id (be32) | escaped(column_value) |
/// primary_key`. The escaped column value is self-terminating, so the primary
/// key is simply the remainder of the bytes.
pub fn index_key(index_id: u32, column_value: &[u8], primary_key: &[u8]) -> Vec<u8> {
    let escaped = escape_column(column_value);
    let mut out = Vec::with_capacity(1 + INDEX_ID_BYTES + escaped.len() + primary_key.len());
    out.push(TAG_INDEX);
    out.extend_from_slice(&index_id.to_be_bytes());
    out.extend_from_slice(&escaped);
    out.extend_from_slice(primary_key);
    out
}

/// Parse an index key and return `(column_value, primary_key)`. The returned
/// `column_value` is the escaped slice; callers can compare it directly because
/// the escaping is order-preserving.
pub fn unpack_index_key(internal: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if internal.is_empty() || internal[0] != TAG_INDEX {
        return None;
    }
    if internal.len() < 1 + INDEX_ID_BYTES + TERMINATOR.len() {
        return None;
    }
    let col_start = 1 + INDEX_ID_BYTES;
    let (col, consumed) = unescape_column(&internal[col_start..])?;
    let pk_start = col_start + consumed;
    if pk_start > internal.len() {
        return None;
    }
    let pk = &internal[pk_start..];
    Some((col, pk.to_vec()))
}

/// Build the inclusive start key for scanning an index.
pub fn index_start(index_id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + INDEX_ID_BYTES);
    out.push(TAG_INDEX);
    out.extend_from_slice(&index_id.to_be_bytes());
    out
}

/// Build the inclusive start key for scanning an index with a lower column
/// bound.
pub fn index_start_with(index_id: u32, column_value: &[u8]) -> Vec<u8> {
    let mut out = index_start(index_id);
    out.extend_from_slice(&escape_column(column_value));
    out
}

/// Return the exclusive end key for scanning an index.
pub fn index_end(index_id: u32) -> Vec<u8> {
    // The next index id is `index_id + 1`, so its start key is the exclusive
    // upper bound for `index_id`.
    index_start(index_id.wrapping_add(1))
}

/// Return the exclusive end key for scanning an index with an upper column
/// bound.
pub fn index_end_with(index_id: u32, column_value: &[u8]) -> Vec<u8> {
    index_start_with(index_id.wrapping_add(1), column_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_round_trip() {
        let ik = primary_key(b"foo");
        assert_eq!(unpack_primary_key(&ik), Some(b"foo".as_slice()));
        assert!(unpack_primary_key(b"xfoo").is_none());
    }

    #[test]
    fn index_ordering_by_column() {
        let k1 = index_key(1, b"alice", b"pk1");
        let k2 = index_key(1, b"bob", b"pk1");
        let k3 = index_key(1, b"bob", b"pk2");
        let k4 = index_key(2, b"a", b"pk1");
        assert!(k1 < k2);
        assert!(k2 < k3);
        assert!(k3 < k4);
    }

    #[test]
    fn index_round_trip() {
        let k = index_key(7, b"col", b"pk");
        let (col, pk) = unpack_index_key(&k).unwrap();
        assert_eq!(col, b"col");
        assert_eq!(pk, b"pk");
    }

    #[test]
    fn index_round_trip_with_nulls() {
        let k = index_key(1, b"a\0b", b"pk");
        let (col, pk) = unpack_index_key(&k).unwrap();
        assert_eq!(col, b"a\0b");
        assert_eq!(pk, b"pk");
    }
}
