//! Crash recovery: replay WAL, rebuild indexes, and restore metadata.

use std::collections::HashMap;

use crate::format::{Metadata, VectorRecord, WalRecord};
use crate::index::VectorIndex;
use crate::options::Quantization;
use crate::storage::VectorStorage;
use crate::wal::VectorWal;

/// Replay all WAL records into storage and metadata.
pub fn replay_wal(
    wal: &VectorWal,
    storage: &VectorStorage,
    key_to_id: &mut HashMap<Vec<u8>, u64>,
    next_id: &mut u64,
) -> crate::Result<()> {
    for record in wal.iter()? {
        match record? {
            WalRecord::Put { key, vector } => {
                let id = key_to_id.get(&key).copied().unwrap_or_else(|| {
                    let id = *next_id;
                    *next_id += 1;
                    key_to_id.insert(key.clone(), id);
                    id
                });
                storage.put(VectorRecord {
                    id,
                    key: key.clone(),
                    vector,
                })?;
            }
            WalRecord::Delete { key } => {
                if let Some(id) = key_to_id.remove(&key) {
                    storage.delete(id);
                }
            }
            WalRecord::Checkpoint { next_id: cp_next } => {
                *next_id = cp_next.max(*next_id);
            }
        }
    }
    Ok(())
}

/// Build or rebuild the ANN index from the current storage contents.
pub fn rebuild_index(
    storage: &VectorStorage,
    index: &mut dyn VectorIndex,
    quantization: Quantization,
) -> crate::Result<()> {
    if quantization == Quantization::Scalar {
        storage.rebuild_quantizer()?;
    }
    let records = storage.records();
    index.build(&records);
    Ok(())
}

/// Recover an engine from WAL and optional persisted pages.
pub fn recover(
    wal: &VectorWal,
    storage: &VectorStorage,
    index: &mut dyn VectorIndex,
    meta: &mut Metadata,
) -> crate::Result<()> {
    storage.load()?;
    replay_wal(wal, storage, &mut meta.key_to_id, &mut meta.next_id)?;
    rebuild_index(storage, index, meta.options.quantization)?;
    Ok(())
}
