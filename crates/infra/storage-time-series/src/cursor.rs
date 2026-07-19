//! Ordered cursor over composite keys for `storage_traits::Engine`.

use bytes::Bytes;
use storage_traits::Cursor;

use crate::format::{Sample, Value, decode_composite_key};

/// Cursor over encoded time-series key/value pairs.
#[derive(Debug)]
pub struct TimeSeriesCursor {
    items: Vec<(Vec<u8>, Vec<u8>)>,
    position: usize,
}

impl TimeSeriesCursor {
    /// Create a cursor from decoded samples encoded as composite keys.
    pub fn new(samples: Vec<Sample>, series_key: &[u8]) -> Self {
        let items = samples
            .into_iter()
            .map(|s| {
                let key = crate::format::encode_composite_key(series_key, s.timestamp);
                (key, s.value.encode())
            })
            .collect();
        Self {
            items,
            position: 0,
        }
    }

    /// Create a cursor from a sorted map of composite key → encoded value.
    pub fn from_map(map: std::collections::BTreeMap<Vec<u8>, Vec<u8>>) -> Self {
        let items: Vec<_> = map.into_iter().collect();
        Self {
            items,
            position: 0,
        }
    }
}

impl Iterator for TimeSeriesCursor {
    type Item = crate::Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.items.len() {
            return None;
        }
        let (k, v) = self.items[self.position].clone();
        self.position += 1;
        Some(Ok((Bytes::from(k), Bytes::from(v))))
    }
}

impl Cursor for TimeSeriesCursor {
    type Error = crate::Error;

    fn seek(&mut self, target: &[u8]) -> crate::Result<()> {
        self.position = self
            .items
            .binary_search_by(|(k, _)| k.as_slice().cmp(target))
            .unwrap_or_else(|i| i);
        Ok(())
    }
}

/// Decode a value from an encoded cursor value.
pub fn decode_cursor_value(value: &[u8]) -> crate::Result<Value> {
    Value::decode(value)
}

/// Decode a composite cursor key into `(series_key, timestamp)`.
pub fn decode_cursor_key(key: &[u8]) -> crate::Result<(Vec<u8>, u64)> {
    let (series_key, ts) = decode_composite_key(key)?;
    Ok((series_key.to_vec(), ts))
}
