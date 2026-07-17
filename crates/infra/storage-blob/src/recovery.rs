//! Recovery: replay the index WAL, rebuild the in-memory index, and truncate
//! any torn record at the end of the active volume.

use std::path::Path;

use storage_wal::Wal;

use crate::index::{BlobLocation, Index};
use crate::index_wal::IndexRecord;
use crate::volume_manager::VolumeManager;
use crate::{Error, Result};

/// Recover the index and volume state on open.
pub fn recover(_path: &Path, wal: &Wal, index: &Index, volumes: &VolumeManager) -> Result<()> {
    // TODO: load binary index snapshot if present, then replay WAL from the
    // snapshot's end LSN.  For now we replay the entire WAL.
    replay_wal(wal, index)?;
    truncate_torn_volume_tail(volumes)?;
    Ok(())
}

fn replay_wal(wal: &Wal, index: &Index) -> Result<()> {
    // LSNs in storage_wal are byte offsets, so replay from the beginning (0).
    let iter = wal.iter(0).map_err(|e| Error::IndexWal(e.to_string()))?;
    for record in iter {
        let record = record.map_err(|e| Error::IndexWal(e.to_string()))?;
        let index_record = IndexRecord::decode(&record.payload)?;
        apply_index_record(index, &index_record);
    }
    Ok(())
}

fn apply_index_record(index: &Index, record: &IndexRecord) {
    match record {
        IndexRecord::Put {
            id,
            volume_number,
            offset,
            payload_len,
            payload_crc,
        } => {
            index.put(
                id.clone(),
                BlobLocation {
                    volume_number: *volume_number,
                    offset: *offset,
                    payload_len: *payload_len,
                    payload_crc: *payload_crc,
                },
            );
        }
        IndexRecord::Delete { id } => {
            index.delete(id);
        }
        IndexRecord::GcMove {
            id,
            new_volume_number,
            new_offset,
            new_payload_len,
            new_payload_crc,
            ..
        } => {
            index.put(
                id.clone(),
                BlobLocation {
                    volume_number: *new_volume_number,
                    offset: *new_offset,
                    payload_len: *new_payload_len,
                    payload_crc: *new_payload_crc,
                },
            );
        }
    }
}

fn truncate_torn_volume_tail(volumes: &VolumeManager) -> Result<()> {
    let Some(active_number) = volumes.active_volume_number() else {
        return Ok(());
    };
    let reader = volumes.reader(active_number)?;
    let file_size = reader.file_size()?;

    let mut offset = 0u64;
    loop {
        if offset + crate::format::HEADER_SIZE as u64 > file_size {
            break;
        }
        match reader.read_record(offset) {
            Ok((header, _id, _payload)) => {
                offset += header.record_size();
            }
            Err(_) => break,
        }
    }

    if offset < file_size {
        // The active volume ends with a partial/corrupt record.  Truncate it.
        volumes.truncate_active_volume(offset)?;
    }

    Ok(())
}

/// Scan all volumes and return any record whose ID is not present in `index`.
/// This is an expensive but useful recovery fallback.
#[allow(dead_code)]
pub fn find_missing_index_entries(
    volumes: &VolumeManager,
    _index: &Index,
) -> Result<Vec<(Vec<u8>, BlobLocation)>> {
    // TODO: enumerate all volume files, iterate records, and return those whose
    // ID is not in the index.  This is a future robustness enhancement.
    let _ = volumes;
    Err(Error::IndexWal("not implemented".into()))
}
