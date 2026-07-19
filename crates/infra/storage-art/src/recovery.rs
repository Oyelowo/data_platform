//! Recovery logic for the durable `ArtEngine`.
//!
//! Recovery loads the most recent consistent snapshot, replays WAL records
//! written after that snapshot, and returns a recovered `ArtMap` plus the
//! current metadata and WAL handle.

use std::path::Path;

use storage_wal::Wal;

use crate::format::{Metadata, WalRecord, file_crc, meta_path, snapshot_path, wal_dir};
use crate::map::ArtMap;
use crate::options::ArtEngineOptions;
use crate::{Error, Result};

/// Recover an engine from `dir` using `options`.
///
/// The caller's options are validated and then merged: metadata on disk takes
/// precedence for the snapshot LSN and CRC, while runtime options are used for
/// the WAL configuration.
pub fn recover(
    dir: impl AsRef<Path>,
    options: ArtEngineOptions,
) -> Result<(ArtMap, Metadata, Wal)> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir)?;

    let meta_file = meta_path(dir);
    let snapshot_file = snapshot_path(dir);
    let wal_dir = wal_dir(dir);

    let mut metadata = if meta_file.exists() {
        let bytes = std::fs::read(&meta_file)?;
        Metadata::decode(&bytes)?
    } else {
        Metadata::new(options.clone())
    };
    // Runtime options control WAL behavior; persisted options control map shape.
    metadata.options.wal_sync_policy = options.wal_sync_policy;
    metadata.options.wal_segment_size = options.wal_segment_size;
    metadata.options.snapshot_on_sync = options.snapshot_on_sync;

    let mut map = ArtMap::new(metadata.options.map.clone());

    // Determine where WAL replay should start. `last_snapshot_lsn` stores the
    // first WAL byte offset that is not covered by the snapshot (i.e. the next
    // record to replay).
    let replay_from: u64 = if snapshot_file.exists() {
        let actual_crc = file_crc(&snapshot_file)?;
        if actual_crc != metadata.snapshot_crc {
            return Err(Error::Corruption(format!(
                "snapshot crc mismatch: expected {:#x}, got {:#x}",
                metadata.snapshot_crc, actual_crc
            )));
        }
        let bytes = std::fs::read(&snapshot_file)?;
        map = crate::snapshot::decode(&bytes)?;
        metadata.last_snapshot_lsn
    } else if metadata.last_snapshot_lsn > 0 {
        return Err(Error::Corruption(format!(
            "metadata references snapshot at lsn {} but snapshot.bin is missing",
            metadata.last_snapshot_lsn
        )));
    } else {
        0
    };

    let wal = Wal::open(&wal_dir, options.wal_options())?;
    replay(&wal, replay_from, &mut map)?;

    Ok((map, metadata, wal))
}

/// Replay WAL records from `start_lsn` into `map`.
fn replay(wal: &Wal, start_lsn: u64, map: &mut ArtMap) -> Result<()> {
    let iter = wal.iter(start_lsn).map_err(Error::Wal)?;
    for result in iter {
        let record = result.map_err(Error::Wal)?;
        // Only application records are replayed. Checkpoint records are metadata
        // markers and do not mutate the tree.
        if record.ty != storage_wal::RecordType::Put {
            continue;
        }
        let wal_record = WalRecord::from_wal(&record)?;
        apply_record(map, wal_record)?;
    }
    Ok(())
}

/// Apply a single logical WAL record to an in-memory map.
fn apply_record(map: &mut ArtMap, record: WalRecord) -> Result<()> {
    match record {
        WalRecord::Put { key, value } => {
            map.insert(&key, &value)?;
        }
        WalRecord::Delete { key } => {
            map.remove(&key)?;
        }
        WalRecord::Batch(ops) => {
            for op in ops {
                match op {
                    crate::format::BatchOp::Put { key, value } => {
                        map.insert(&key, &value)?;
                    }
                    crate::format::BatchOp::Delete { key } => {
                        map.remove(&key)?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{WalRecord, write_atomic};
    use crate::options::ArtEngineOptions;

    #[test]
    fn recover_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let (map, meta, _wal) = recover(dir.path(), ArtEngineOptions::default()).unwrap();
        assert!(map.is_empty());
        assert_eq!(meta.last_snapshot_lsn, 0);
    }

    #[test]
    fn recover_from_snapshot_only() {
        let dir = tempfile::tempdir().unwrap();
        let map = ArtMap::new(crate::options::ArtMapOptions::default());
        map.insert(b"a", b"1").unwrap();
        let snap = crate::snapshot::encode(&map).unwrap();
        write_atomic(snapshot_path(dir.path()), &snap).unwrap();

        let mut meta = Metadata::new(ArtEngineOptions::default());
        meta.snapshot_crc = storage_format::crc32c(&snap);
        meta.last_snapshot_lsn = 5;
        write_atomic(meta_path(dir.path()), &meta.encode()).unwrap();

        let (recovered, _, _wal) = recover(dir.path(), ArtEngineOptions::default()).unwrap();
        assert_eq!(recovered.get(b"a"), Some(bytes::Bytes::from_static(b"1")));
    }

    #[test]
    fn recover_replays_wal() {
        let dir = tempfile::tempdir().unwrap();
        let wal = Wal::open(wal_dir(dir.path()), storage_wal::WalOptions::default()).unwrap();
        let rec = WalRecord::Put {
            key: b"k".to_vec(),
            value: b"v".to_vec(),
        }
        .into_wal();
        wal.append_record(rec, storage_wal::Durability::Immediate)
            .unwrap();
        wal.close().unwrap();

        let (map, _, _wal) = recover(dir.path(), ArtEngineOptions::default()).unwrap();
        assert_eq!(map.get(b"k"), Some(bytes::Bytes::from_static(b"v")));
    }
}
