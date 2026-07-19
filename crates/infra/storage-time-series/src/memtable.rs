//! In-memory write buffer for recent samples.

use std::collections::BTreeMap;

use crate::chunk::builder::ChunkBuilder;
use crate::format::{Sample, Timestamp};
use crate::options::{CompressionKind, RetentionPolicy};

/// In-memory write buffer keyed by `(series_key, timestamp)`.
#[derive(Debug, Clone, Default)]
pub struct MemTable {
    data: BTreeMap<(Vec<u8>, Timestamp), Sample>,
    bytes: usize,
}

impl MemTable {
    /// Create an empty memtable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or overwrite a sample.
    pub fn insert(&mut self, series_key: Vec<u8>, sample: Sample) {
        let key = (series_key, sample.timestamp);
        let value_bytes = sample.value.encode().len();
        let sample_size = 8 + value_bytes;
        if let Some(old) = self.data.remove(&key) {
            self.bytes = self.bytes.saturating_sub(8 + old.value.encode().len());
        }
        self.bytes += sample_size;
        self.data.insert(key, sample);
    }

    /// Delete all samples for a series.
    pub fn delete_series(&mut self, series_key: &[u8]) {
        let keys: Vec<_> = self
            .data
            .range((series_key.to_vec(), 0)..)
            .take_while(|((k, _), _)| k.as_slice() == series_key)
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            if let Some(old) = self.data.remove(&k) {
                self.bytes = self.bytes.saturating_sub(8 + old.value.encode().len());
            }
        }
    }

    /// Delete samples in a half-open time range for a series.
    pub fn delete_range(&mut self, series_key: &[u8], start: Timestamp, end: Timestamp) {
        let keys: Vec<_> = self
            .data
            .range((series_key.to_vec(), start)..(series_key.to_vec(), end))
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            if let Some(old) = self.data.remove(&k) {
                self.bytes = self.bytes.saturating_sub(8 + old.value.encode().len());
            }
        }
    }

    /// Return all samples for a series in time order.
    pub fn series(&self, series_key: &[u8]) -> Vec<Sample> {
        self.data
            .range((series_key.to_vec(), 0)..)
            .take_while(|((k, _), _)| k.as_slice() == series_key)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Return samples for a series in the half-open range `[start, end)`.
    pub fn range(&self, series_key: &[u8], start: Timestamp, end: Timestamp) -> Vec<Sample> {
        self.data
            .range((series_key.to_vec(), start)..(series_key.to_vec(), end))
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Return the latest sample for a series, if any.
    pub fn latest(&self, series_key: &[u8]) -> Option<Sample> {
        self.data
            .range((series_key.to_vec(), 0)..)
            .take_while(|((k, _), _)| k.as_slice() == series_key)
            .last()
            .map(|(_, v)| v.clone())
    }

    /// Approximate byte size of buffered samples.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Number of buffered samples.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the memtable is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Drain the memtable into per-series chunk builders, applying retention.
    ///
    /// Returns `(builders, retained_memtable_samples)`.
    pub fn flush(
        &mut self,
        compression: CompressionKind,
        chunk_size_target: usize,
        retention: Option<RetentionPolicy>,
        now: Timestamp,
    ) -> (Vec<ChunkBuilder>, MemTable) {
        let mut by_series: BTreeMap<Vec<u8>, Vec<Sample>> = BTreeMap::new();
        let retained = MemTable::new();
        for ((series_key, _ts), sample) in &self.data {
            if should_retain(sample.timestamp, retention, now) {
                by_series
                    .entry(series_key.clone())
                    .or_default()
                    .push(sample.clone());
            }
        }
        let mut builders = Vec::new();
        for (series_key, mut samples) in by_series {
            samples.sort_by_key(|s| s.timestamp);
            if let Some(RetentionPolicy::MaxSamples(limit)) = retention
                && samples.len() > limit
            {
                let start = samples.len() - limit;
                samples = samples.split_off(start);
            }
            let mut builder = ChunkBuilder::new(series_key.clone(), compression);
            let mut current_size = 0usize;
            for sample in samples {
                let sample_size = 8 + sample.value.encode().len();
                if !builder.is_empty()
                    && current_size + sample_size > chunk_size_target
                {
                    builders.push(builder);
                    builder = ChunkBuilder::new(series_key.clone(), compression);
                    current_size = 0;
                }
                builder.push(sample.clone()).ok();
                current_size += sample_size;
            }
            if !builder.is_empty() {
                builders.push(builder);
            }
        }
        self.clear();
        (builders, retained)
    }

    /// Remove all buffered samples.
    pub fn clear(&mut self) {
        self.data.clear();
        self.bytes = 0;
    }

    /// Apply retention to samples already in the memtable.
    pub fn apply_retention(&mut self, retention: Option<RetentionPolicy>, now: Timestamp) {
        if retention.is_none() {
            return;
        }
        let keys: Vec<_> = self
            .data
            .iter()
            .filter(|(_, sample)| !should_retain(sample.timestamp, retention, now))
            .map(|(k, _)| k.clone())
            .collect();
        for k in keys {
            if let Some(old) = self.data.remove(&k) {
                self.bytes = self.bytes.saturating_sub(8 + old.value.encode().len());
            }
        }
    }

    /// Return an iterator over all buffered samples.
    pub fn iter(&self) -> impl Iterator<Item = (&(Vec<u8>, Timestamp), &Sample)> {
        self.data.iter()
    }
}

fn should_retain(ts: Timestamp, retention: Option<RetentionPolicy>, now: Timestamp) -> bool {
    match retention {
        None => true,
        Some(RetentionPolicy::Duration(d)) => {
            let nanos = d.as_nanos() as u64;
            ts.saturating_add(nanos) >= now
        }
        Some(RetentionPolicy::MaxSamples(_)) => true, // handled per-series at flush time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;

    fn sample(ts: Timestamp, v: f64) -> Sample {
        Sample {
            timestamp: ts,
            value: Value::F64(v),
        }
    }

    #[test]
    fn insert_and_series() {
        let mut mt = MemTable::new();
        mt.insert(b"cpu".to_vec(), sample(1, 1.0));
        mt.insert(b"cpu".to_vec(), sample(2, 2.0));
        mt.insert(b"mem".to_vec(), sample(1, 10.0));
        assert_eq!(mt.series(b"cpu").len(), 2);
        assert_eq!(mt.series(b"mem").len(), 1);
    }

    #[test]
    fn overwrite_same_timestamp() {
        let mut mt = MemTable::new();
        mt.insert(b"cpu".to_vec(), sample(1, 1.0));
        mt.insert(b"cpu".to_vec(), sample(1, 2.0));
        assert_eq!(mt.len(), 1);
        assert_eq!(mt.latest(b"cpu").unwrap().value, Value::F64(2.0));
    }

    #[test]
    fn flush_creates_chunks() {
        let mut mt = MemTable::new();
        for i in 0..100u64 {
            mt.insert(b"cpu".to_vec(), sample(i, i as f64));
        }
        let (builders, _retained) = mt.flush(CompressionKind::Gorilla, 256, None, 0);
        assert!(!builders.is_empty());
        assert!(mt.is_empty());
    }

    #[test]
    fn retention_duration() {
        let mut mt = MemTable::new();
        mt.insert(b"cpu".to_vec(), sample(0, 1.0));
        mt.insert(b"cpu".to_vec(), sample(100, 2.0));
        mt.apply_retention(
            Some(RetentionPolicy::Duration(std::time::Duration::from_nanos(50))),
            100,
        );
        assert_eq!(mt.len(), 1);
        assert_eq!(mt.latest(b"cpu").unwrap().timestamp, 100);
    }
}
