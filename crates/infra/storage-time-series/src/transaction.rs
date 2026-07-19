//! Transaction support for the time-series engine.

use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use storage_traits::{Cursor, Engine, IsolationLevel, Result as TraitResult, Transaction, TxnOptions};

use crate::cursor::TimeSeriesCursor;
use crate::engine::TimeSeriesEngine;
use crate::error::Error;
use crate::format::{Timestamp, Value, decode_composite_key, encode_composite_key};

/// A transaction over a [`TimeSeriesEngine`].
#[derive(Debug)]
pub struct TimeSeriesTransaction {
    engine: TimeSeriesEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local_puts: BTreeMap<Vec<u8>, Value>, // composite key -> value
    local_deletes: HashSet<Vec<u8>>,      // composite keys
    series_deletes: HashSet<Vec<u8>>,     // series keys
    range_deletes: Vec<(Vec<u8>, Timestamp, Timestamp)>, // series key, start, end
}

impl TimeSeriesTransaction {
    pub(crate) fn new(engine: TimeSeriesEngine, opts: TxnOptions) -> Self {
        Self {
            engine,
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local_puts: BTreeMap::new(),
            local_deletes: HashSet::new(),
            series_deletes: HashSet::new(),
            range_deletes: Vec::new(),
        }
    }

    fn ensure_active(&self) -> crate::Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }

    fn value_in_range_delete(&self, series_key: &[u8], ts: Timestamp) -> bool {
        for (k, start, end) in &self.range_deletes {
            if k.as_slice() == series_key && ts >= *start && ts < *end {
                return true;
            }
        }
        false
    }
}

impl Transaction for TimeSeriesTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        self.ensure_active()?;
        if self.local_deletes.contains(key) {
            return Ok(None);
        }
        let (series_key, ts) = decode_composite_key(key)?;
        let series_key_vec = series_key.to_vec();
        if self.series_deletes.contains(&series_key_vec)
            || self.value_in_range_delete(&series_key_vec, ts)
        {
            return Ok(None);
        }
        if let Some(value) = self.local_puts.get(key) {
            return Ok(Some(Bytes::from(value.encode())));
        }
        self.engine.get(key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let value = Value::decode(value)?;
        self.local_puts.insert(key.to_vec(), value);
        self.local_deletes.remove(key);
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(key.to_vec());
        self.local_puts.remove(key);
        Ok(())
    }

    fn scan(
        &self,
        start: Option<&[u8]>,
        end: Option<&[u8]>,
    ) -> TraitResult<impl Cursor<Error = Self::Error>, Self::Error> {
        self.ensure_active()?;
        let mut map: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        // Base engine scan.
        let engine_cursor = Engine::scan(&self.engine, start, end)?;
        for item in engine_cursor {
            let (k, v) = item?;
            map.insert(k.to_vec(), v.to_vec());
        }

        // Apply deletes.
        for k in &self.local_deletes {
            map.remove(k);
        }

        // Apply series deletes and range deletes.
        let to_remove: Vec<_> = map
            .keys()
            .filter(|k| {
                if let Ok((series_key, ts)) = decode_composite_key(k) {
                    let series_key_vec = series_key.to_vec();
                    if self.series_deletes.contains(&series_key_vec) {
                        return true;
                    }
                    if self.value_in_range_delete(&series_key_vec, ts) {
                        return true;
                    }
                }
                false
            })
            .cloned()
            .collect();
        for k in to_remove {
            map.remove(&k);
        }

        // Apply puts.
        for (k, v) in &self.local_puts {
            let include = {
                let above_start = start.map(|s| k.as_slice() >= s).unwrap_or(true);
                let below_end = end.map(|e| k.as_slice() < e).unwrap_or(true);
                above_start && below_end
            };
            if include {
                map.insert(k.clone(), v.encode());
            }
        }

        Ok(TimeSeriesCursor::from_map(map))
    }

    fn commit(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        let _guard = self.engine.inner().write_lock.lock();
        for key in self.local_deletes {
            let (series_key, ts) = decode_composite_key(&key)?;
            self.engine.delete_range_unlocked(series_key, ts, ts + 1)?;
        }
        for (series_key, start, end) in self.range_deletes {
            self.engine.delete_range_unlocked(&series_key, start, end)?;
        }
        for series_key in self.series_deletes {
            self.engine.delete_series_unlocked(&series_key)?;
        }
        for (key, value) in self.local_puts {
            let (series_key, timestamp) = decode_composite_key(&key)?;
            self.engine.put_unlocked(series_key.to_vec(), timestamp, value)?;
        }
        self.active = false;
        Ok(())
    }

    fn rollback(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.active = false;
        Ok(())
    }

    fn set_isolation(&mut self, level: IsolationLevel) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        self.isolation = level;
        Ok(())
    }
}

impl TimeSeriesTransaction {
    /// Delete all samples for a series within this transaction.
    pub fn delete_series(&mut self, series_key: &[u8]) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.series_deletes.insert(series_key.to_vec());
        // Remove buffered puts for this series.
        let keys: Vec<_> = self
            .local_puts
            .keys()
            .filter(|k| {
                decode_composite_key(k)
                    .map(|(sk, _)| sk == series_key)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for k in keys {
            self.local_puts.remove(&k);
        }
        Ok(())
    }

    /// Delete a time range for a series within this transaction.
    pub fn delete_range(
        &mut self,
        series_key: &[u8],
        start: Timestamp,
        end: Timestamp,
    ) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.range_deletes
            .push((series_key.to_vec(), start, end));
        // Remove buffered puts in this range.
        let keys: Vec<_> = self
            .local_puts
            .keys()
            .filter(|k| {
                decode_composite_key(k)
                    .map(|(sk, ts)| sk == series_key && ts >= start && ts < end)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        for k in keys {
            self.local_puts.remove(&k);
        }
        Ok(())
    }

    /// Insert a typed sample into the transaction.
    pub fn put_sample(
        &mut self,
        series_key: Vec<u8>,
        timestamp: Timestamp,
        value: Value,
    ) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let key = encode_composite_key(&series_key, timestamp);
        self.local_puts.insert(key, value);
        Ok(())
    }
}
