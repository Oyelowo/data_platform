//! Crash recovery: rebuild live state from the feature store and replay WAL.

use crate::feature::Feature;
use crate::format::{FeatureRecord, Metadata, WalRecord};
use crate::store::{FeatureAddress, FeatureStore};
use crate::wal::GeoWal;

/// Rebuild the live id→address map by scanning the feature store.
///
/// Later records win, so the map reflects the last inserted version of each
/// feature.
pub fn rebuild_live_map_from_store(
    store: &FeatureStore,
    metadata: &mut Metadata,
) -> crate::Result<()> {
    metadata.live.clear();
    let mut offset = 0u64;
    for result in store.iter()? {
        let feature = result?;
        let record_bytes = FeatureRecord::encode(&feature)?;
        let address = FeatureAddress {
            file_id: metadata.store_file_id,
            offset,
            len: record_bytes.len() as u32,
        };
        metadata.live.insert(feature.id, address);
        offset += record_bytes.len() as u64;
    }
    Ok(())
}

/// Replay WAL records that are newer than the last checkpoint.
pub fn replay_wal(
    wal: &GeoWal,
    store: &FeatureStore,
    metadata: &mut Metadata,
) -> crate::Result<()> {
    let checkpoint_lsn = metadata.wal_checkpoint_lsn.unwrap_or(0);
    for res in wal.iter()? {
        let (lsn, record) = res?;
        if lsn <= checkpoint_lsn {
            // Records at or before the checkpoint are already reflected in the
            // loaded store/index.
            continue;
        }
        apply_wal_record(record, store, metadata)?;
    }
    Ok(())
}

fn apply_wal_record(
    record: WalRecord,
    store: &FeatureStore,
    metadata: &mut Metadata,
) -> crate::Result<()> {
    match record {
        WalRecord::InsertFeature {
            id,
            geometry,
            properties,
        } => {
            let geometry = crate::wkb::decode(&geometry)?;
            let properties = serde_json::from_slice(&properties)
                .map_err(|e| crate::Error::property_encoding(e.to_string()))?;
            let feature = Feature::new(id.clone(), geometry, properties);
            let address = store.insert(&feature)?;
            if let Some(old) = metadata.live.insert(id, address) {
                metadata.stale_bytes += old.len as u64;
            }
        }
        WalRecord::DeleteFeature { id } => {
            if let Some(old) = metadata.live.remove(&id) {
                metadata.stale_bytes += old.len as u64;
            }
        }
        WalRecord::UpdateProperties { id, properties } => {
            let properties = serde_json::from_slice(&properties)
                .map_err(|e| crate::Error::property_encoding(e.to_string()))?;
            let current_address = match metadata.live.get(&id).copied() {
                Some(a) => a,
                None => return Ok(()),
            };
            let current = match store.get(current_address)? {
                Some(f) => f,
                None => return Ok(()),
            };
            let feature = Feature::new(id.clone(), current.geometry, properties);
            let address = store.insert(&feature)?;
            if let Some(old) = metadata.live.insert(id, address) {
                metadata.stale_bytes += old.len as u64;
            }
        }
    }
    Ok(())
}
