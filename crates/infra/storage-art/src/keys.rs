//! Key and prefix helpers for the Adaptive Radix Trie.
//!
//! Keys are opaque byte sequences. Ordering is lexicographic over `u8` bytes.
//! Prefix compression collapses runs of single-child nodes by storing the
//! shared prefix directly in the parent node.

/// The maximum prefix length stored inline in a node. Longer common prefixes
/// are split into chained inner nodes, matching the ART design.
pub const MAX_PREFIX_LEN: usize = 255;

/// Compute the length of the common prefix between `a` and `b` starting at
/// `offset` in `a`. Returns `(common_len, diverging_byte_in_b)` where
/// `diverging_byte_in_b` is only meaningful if `b` is longer than the common
/// prefix and `offset + common_len < a.len()`.
pub fn common_prefix_len(a: &[u8], b: &[u8], offset: usize) -> usize {
    let a = &a[offset..];
    let b = &b[offset..];
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

/// Compare `key` starting at `depth` against `prefix`. Returns the number of
/// matching bytes. If the returned length equals `prefix.len()`, the prefix
/// fully matches.
pub fn match_prefix(prefix: &[u8], key: &[u8], depth: usize) -> usize {
    let key = &key[depth..];
    prefix
        .iter()
        .zip(key.iter())
        .take_while(|(p, k)| p == k)
        .count()
}

/// Determine the next diverging byte in `key` at `depth`, if any.
pub fn diverging_byte(key: &[u8], depth: usize) -> Option<u8> {
    key.get(depth).copied()
}

/// Clamp a prefix slice to the maximum inline length.
pub fn truncate_prefix(prefix: &[u8]) -> &[u8] {
    if prefix.len() > MAX_PREFIX_LEN {
        &prefix[..MAX_PREFIX_LEN]
    } else {
        prefix
    }
}

/// Compare two byte keys lexicographically.
pub fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_prefix_basic() {
        assert_eq!(common_prefix_len(b"foobar", b"foobaz", 0), 5);
        assert_eq!(common_prefix_len(b"foobar", b"", 0), 0);
        assert_eq!(common_prefix_len(b"", b"foo", 0), 0);
    }

    #[test]
    fn common_prefix_with_offset() {
        assert_eq!(common_prefix_len(b"abcdef", b"cdefgh", 2), 0);
    }

    #[test]
    fn match_prefix_full_and_partial() {
        assert_eq!(match_prefix(b"foo", b"foobar", 0), 3);
        assert_eq!(match_prefix(b"foo", b"fog", 0), 2);
        assert_eq!(match_prefix(b"foo", b"bar", 0), 0);
    }

    #[test]
    fn diverging_byte_basic() {
        assert_eq!(diverging_byte(b"abc", 2), Some(b'c'));
        assert_eq!(diverging_byte(b"abc", 3), None);
    }

    #[test]
    fn truncate_prefix_caps_length() {
        let big = vec![b'x'; MAX_PREFIX_LEN + 10];
        assert_eq!(truncate_prefix(&big).len(), MAX_PREFIX_LEN);
    }
}
