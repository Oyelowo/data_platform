//! Range and prefix cursors for `ArtMap`.
//!
//! The current implementation materializes a consistent, sorted snapshot of the
//! matching entries. This keeps the cursor simple and correct while still
//! satisfying the strict ascending-order requirement.

use bytes::Bytes;
use storage_traits::{Cursor, Error, Result};

use crate::map::ArtMap;

/// An iterator over a sorted range or prefix of keys in an `ArtMap`.
#[derive(Debug)]
pub struct ArtCursor {
    entries: Vec<(Bytes, Bytes)>,
    position: usize,
}

impl ArtCursor {
    /// Create a cursor from an already-filtered, sorted buffer.
    pub(crate) fn from_snapshot(buffer: Vec<(Bytes, Bytes)>) -> Self {
        Self {
            entries: buffer,
            position: 0,
        }
    }

    /// Create a cursor over all keys in `[start, end)`.
    pub(crate) fn range(map: &ArtMap, start: Option<&[u8]>, end: Option<&[u8]>) -> Self {
        let mut entries = Vec::new();
        map.collect_entries(&mut entries);
        entries.retain(|(k, _)| {
            let k = k.as_ref();
            let above_start = start.map(|s| k >= s).unwrap_or(true);
            let below_end = end.map(|e| k < e).unwrap_or(true);
            above_start && below_end
        });
        Self {
            entries,
            position: 0,
        }
    }

    /// Create a cursor over all keys starting with `prefix`.
    pub(crate) fn prefix(map: &ArtMap, prefix: &[u8]) -> Self {
        let mut entries = Vec::new();
        map.collect_entries(&mut entries);
        entries.retain(|(k, _)| k.as_ref().starts_with(prefix));
        Self {
            entries,
            position: 0,
        }
    }
}

impl Iterator for ArtCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position < self.entries.len() {
            let item = self.entries[self.position].clone();
            self.position += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

impl Cursor for ArtCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.position = self.entries.partition_point(|(k, _)| k.as_ref() < target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ArtMapOptions;

    #[test]
    fn cursor_sorted() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"c", b"3").unwrap();
        map.insert(b"a", b"1").unwrap();
        map.insert(b"b", b"2").unwrap();
        let keys: Vec<_> = ArtCursor::range(&map, None, None)
            .map(|r| r.unwrap().0)
            .collect();
        assert_eq!(
            keys,
            vec![
                Bytes::from_static(b"a"),
                Bytes::from_static(b"b"),
                Bytes::from_static(b"c")
            ]
        );
    }

    #[test]
    fn cursor_prefix() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"prefix:one", b"1").unwrap();
        map.insert(b"prefix:two", b"2").unwrap();
        map.insert(b"other", b"3").unwrap();
        let keys: Vec<_> = ArtCursor::prefix(&map, b"prefix:")
            .map(|r| r.unwrap().0)
            .collect();
        assert_eq!(
            keys,
            vec![
                Bytes::from_static(b"prefix:one"),
                Bytes::from_static(b"prefix:two")
            ]
        );
    }

    #[test]
    fn cursor_seek() {
        let map = ArtMap::new(ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        map.insert(b"c", b"3").unwrap();
        let mut cursor = ArtCursor::range(&map, None, None);
        cursor.seek(b"b").unwrap();
        assert_eq!(cursor.next().unwrap().unwrap().0, Bytes::from_static(b"c"));
    }
}
