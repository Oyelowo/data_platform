//! Store compaction: rewrite the feature store to remove deleted/updated garbage.

use std::path::Path;

use crate::format::Metadata;
use crate::store::FeatureStore;

/// Rewrite the feature store, retaining only live features.
///
/// `old_store` is the current store. The function creates a new store file with
/// the next file id, copies live features, updates `metadata.live`, and removes
/// the old store file. On success the new store is returned.
pub fn compact(
    dir: impl AsRef<Path>,
    metadata: &mut Metadata,
    old_store: &FeatureStore,
) -> crate::Result<FeatureStore> {
    let dir = dir.as_ref();
    let new_file_id = metadata.store_file_id.wrapping_add(1);
    let new_store = FeatureStore::open(dir, new_file_id)?;

    // Copy live features in deterministic id order.
    let live_ids: Vec<Vec<u8>> = metadata.live.keys().cloned().collect();
    let mut new_addresses = std::collections::BTreeMap::new();
    for id in live_ids {
        let old_address = metadata
            .live
            .get(&id)
            .copied()
            .ok_or_else(|| crate::Error::not_found("live feature disappeared during compaction"))?;
        let feature = old_store
            .get(old_address)?
            .ok_or_else(|| crate::Error::corruption("live feature missing from store"))?;
        let new_address = new_store.insert(&feature)?;
        new_addresses.insert(id, new_address);
    }

    // Switch metadata to the new store.
    metadata.store_file_id = new_file_id;
    metadata.live = new_addresses;
    metadata.stale_bytes = 0;
    new_store.sync()?;

    // Remove the old store file; it is no longer referenced by metadata.
    let old_path = old_store.path();
    if old_path.exists() {
        let _ = std::fs::remove_file(&old_path);
    }

    Ok(new_store)
}
