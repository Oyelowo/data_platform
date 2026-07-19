//! Transactions for the geospatial engine.

use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use storage_traits::{Cursor, Engine, IsolationLevel, Result as TraitResult, Transaction, TxnOptions};

use crate::cursor::GeoCursor;
use crate::engine::GeoEngine;
use crate::error::Error;
use crate::feature::{Geometry, PropertyMap};
use crate::format::{decode_feature_value, decode_key, decode_property_value, encode_feature_value};

/// A transaction over a [`GeoEngine`](crate::engine::GeoEngine).
pub struct GeoTransaction {
    engine: GeoEngine,
    read_only: bool,
    isolation: IsolationLevel,
    active: bool,
    local_puts: BTreeMap<Vec<u8>, Vec<u8>>,
    local_deletes: HashSet<Vec<u8>>,
}

impl GeoTransaction {
    pub(crate) fn new(engine: GeoEngine, opts: TxnOptions) -> Self {
        Self {
            engine,
            read_only: opts.read_only,
            isolation: opts.isolation,
            active: true,
            local_puts: BTreeMap::new(),
            local_deletes: HashSet::new(),
        }
    }

    fn ensure_active(&self) -> crate::Result<()> {
        if !self.active {
            return Err(Error::InactiveTransaction);
        }
        Ok(())
    }
}

impl Transaction for GeoTransaction {
    type Error = Error;

    fn get(&self, key: &[u8]) -> TraitResult<Option<Bytes>, Self::Error> {
        self.ensure_active()?;
        if self.local_deletes.contains(key) {
            return Ok(None);
        }
        if let Some(value) = self.local_puts.get(key) {
            return Ok(Some(Bytes::from(value.clone())));
        }
        self.engine.get(key)
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_puts.insert(key.to_vec(), value.to_vec());
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

        for item in self.engine.scan(start, end)? {
            let (k, v) = item?;
            map.insert(k.to_vec(), v.to_vec());
        }

        for key in &self.local_deletes {
            map.remove(key);
        }

        for (key, value) in &self.local_puts {
            let in_range = {
                let above_start = start.map(|s| key.as_slice() >= s).unwrap_or(true);
                let below_end = end.map(|e| key.as_slice() < e).unwrap_or(true);
                above_start && below_end
            };
            if in_range {
                map.insert(key.clone(), value.clone());
            }
        }

        Ok(GeoCursor::new(map))
    }

    fn commit(mut self) -> TraitResult<(), Self::Error> {
        self.ensure_active()?;
        let _guard = self.engine.inner().write_lock.lock();

        // Apply deletes first so that a subsequent put in the same transaction
        // re-inserts cleanly.
        for key in self.local_deletes {
            match decode_key(&key)? {
                (id, None) => {
                    self.engine.delete_feature_unlocked(id)?;
                }
                (id, Some(property_key)) => {
                    let mut feature = match self.engine.get_feature(id)? {
                        Some(f) => f,
                        None => continue,
                    };
                    feature.properties.remove(property_key);
                    self.engine.insert_feature_unlocked(feature)?;
                }
            }
        }

        for (key, value) in self.local_puts {
            match decode_key(&key)? {
                (id, None) => {
                    let feature = decode_feature_value(&value)?;
                    if feature.id != id {
                        return Err(Error::InvalidArgument(
                            "feature id in key does not match encoded value".into(),
                        ));
                    }
                    self.engine.insert_feature_unlocked(feature)?;
                }
                (id, Some(property_key)) => {
                    self.engine
                        .update_properties_unlocked(id, property_key, decode_property_value(&value))?;
                }
            }
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

impl GeoTransaction {
    /// Typed insert of a feature within this transaction.
    pub fn insert_feature(
        &mut self,
        id: impl Into<Vec<u8>>,
        geometry: Geometry,
        properties: PropertyMap,
    ) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        let id = id.into();
        geometry.validate()?;
        let feature = crate::feature::Feature::new(id.clone(), geometry, properties);
        let key = crate::format::encode_id_key(&id);
        let value = encode_feature_value(&feature)?;
        self.local_puts.insert(key, value);
        self.local_deletes.remove(&crate::format::encode_id_key(&id));
        Ok(())
    }

    /// Typed delete of a feature within this transaction.
    pub fn delete_feature(&mut self, id: &[u8]) -> crate::Result<()> {
        self.ensure_active()?;
        if self.read_only {
            return Err(Error::ReadOnlyTransaction);
        }
        self.local_deletes.insert(crate::format::encode_id_key(id));
        Ok(())
    }
}
