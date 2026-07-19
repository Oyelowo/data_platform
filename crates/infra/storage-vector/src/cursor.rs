//! Ordered scan cursor over vector keys.

use std::collections::BTreeMap;

use bytes::Bytes;
use storage_traits::Cursor;

use crate::error::Error;

/// Cursor over the byte-key / vector-value space of a vector engine.
pub struct VectorCursor {
    data: Vec<(Bytes, Bytes)>,
    pos: usize,
}

impl VectorCursor {
    /// Create a cursor from a sorted key-to-vector map.
    pub fn new(map: BTreeMap<Vec<u8>, Vec<f32>>) -> Self {
        let data: Vec<(Bytes, Bytes)> = map
            .into_iter()
            .map(|(k, v)| {
                let value = crate::format::encode_f32_vec(&v);
                (Bytes::from(k), Bytes::from(value))
            })
            .collect();
        Self { data, pos: 0 }
    }
}

impl Iterator for VectorCursor {
    type Item = crate::Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.data.len() {
            let item = self.data[self.pos].clone();
            self.pos += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

impl Cursor for VectorCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> crate::Result<()> {
        self.pos = self
            .data
            .binary_search_by(|(k, _)| k.as_ref().cmp(target))
            .unwrap_or_else(|i| i);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_order_and_seek() {
        let mut map = BTreeMap::new();
        map.insert(b"a".to_vec(), vec![1.0f32]);
        map.insert(b"b".to_vec(), vec![2.0f32]);
        map.insert(b"c".to_vec(), vec![3.0f32]);
        let mut cursor = VectorCursor::new(map);
        cursor.seek(b"b").unwrap();
        assert_eq!(cursor.next().unwrap().unwrap().0, Bytes::from_static(b"b"));
    }
}
