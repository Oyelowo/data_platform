//! Recovery: reopen an LSM engine from disk.

use std::path::Path;

use crate::manifest::Manifest;
use crate::memtable::MemTable;
use crate::options::LsmOptions;
use crate::wal::{WalRecord, WalRecordType};
use crate::{Result, SequenceNumber};

/// Recover the VersionSet and WAL state from disk. Returns the replayed
/// MemTable and the next sequence number to assign.
pub fn recover(
    db_path: &Path,
    options: &LsmOptions,
    version_set: &crate::version_set::VersionSet,
) -> Result<(MemTable, SequenceNumber)> {
    // 1. Recover MANIFEST if it exists.
    let current_path = db_path.join("CURRENT");
    if current_path.exists() {
        let manifest_name = std::fs::read_to_string(&current_path)?;
        let manifest_file = db_path.join(manifest_name.trim());
        if manifest_file.exists() {
            let edits = Manifest::recover(&manifest_file)?;
            for edit in edits {
                version_set.apply(edit)?;
            }
        }
    }

    // Defensive: ensure the next file number is beyond any existing SSTable so
    // we never overwrite a file referenced by the recovered version.
    let max_file = version_set
        .current()
        .levels
        .iter()
        .flat_map(|level| level.iter().map(|f| f.number))
        .max()
        .unwrap_or(0);
    version_set.set_next_file_number(max_file + 1);

    // 2. Replay WAL.
    let wal = storage_wal::Wal::open(
        db_path.join("wal"),
        storage_wal::WalOptions {
            segment_size: options.wal_segment_size,
            ..Default::default()
        },
    )?;
    let mem = MemTable::new();
    let mut max_seq: Option<SequenceNumber> = None;

    for record in wal.iter(0)? {
        let record = record?;
        let (rec, _) = WalRecord::decode(&record.payload).ok_or_else(|| {
            crate::Error::Corruption("bad wal record during recovery".into())
        })?;

        max_seq = Some(max_seq.map_or(rec.sequence, |m| m.max(rec.sequence)));

        match rec.ty {
            WalRecordType::Put => {
                let value = rec.value.as_ref().map(|v| v.as_ref()).unwrap_or(&[]);
                mem.put(&rec.key, rec.sequence, value);
            }
            WalRecordType::Delete => {
                mem.delete(&rec.key, rec.sequence);
            }
        }
    }

    drop(wal);

    Ok((mem, max_seq.unwrap_or(0)))
}
