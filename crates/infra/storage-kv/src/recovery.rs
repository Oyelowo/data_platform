//! Recovery: reopen an LSM engine from disk.

use std::path::Path;

use crate::blob::BlobStore;
use crate::column_family::ColumnFamilySet;
use crate::manifest::Manifest;
use crate::options::LsmOptions;
use crate::wal::{WalRecord, WalRecordType};
use crate::{Result, SequenceNumber};

/// Recover the VersionSet and WAL state from disk.
///
/// Manifest edits are applied to the column family recorded in the edit.  WAL
/// records are replayed into the column family they belong to; if that family
/// no longer exists the record is replayed into the default family so that no
/// data is silently lost.
pub fn recover(
    db_path: &Path,
    options: &LsmOptions,
    cf_set: &mut ColumnFamilySet,
    blob_store: &BlobStore,
) -> Result<SequenceNumber> {
    let mut max_seq: Option<SequenceNumber> = None;

    // 1. Recover MANIFEST if it exists.
    let current_path = db_path.join("CURRENT");
    if current_path.exists() {
        let manifest_name = std::fs::read_to_string(&current_path)?;
        let manifest_file = db_path.join(manifest_name.trim());
        if manifest_file.exists() {
            let edits = Manifest::recover(&manifest_file)?;
            for edit in edits {
                for (cf_id, name) in &edit.created_cfs {
                    let _ = cf_set.create_with_id(*cf_id, name, options.clone());
                }
                for cf_id in &edit.dropped_cfs {
                    let _ = cf_set.drop(*cf_id);
                }
                let cf_id = edit.cf_id;
                let cf = if let Some(cf) = cf_set.get_mut(cf_id) {
                    cf
                } else {
                    cf_set.default_mut()
                };
                let edit_last_sequence = edit.last_sequence;
                cf.version_set.apply(edit)?;
                max_seq = Some(max_seq.map_or(edit_last_sequence, |m| m.max(edit_last_sequence)));
            }
        }
    }

    // Defensive: ensure the next file number is beyond any existing SSTable so
    // we never overwrite a file referenced by the recovered version.  We use
    // the default CF's version set here because all column families share the
    // same global file-number allocator.
    let max_file = cf_set
        .default()
        .version_set
        .current()
        .levels
        .iter()
        .flat_map(|level| level.iter().map(|f| f.number))
        .max()
        .unwrap_or(0);
    cf_set
        .default()
        .version_set
        .set_next_file_number(max_file + 1);

    // 2. Replay WAL.
    let wal = storage_wal::Wal::open(
        db_path.join("wal"),
        storage_wal::WalOptions {
            segment_size: options.wal_segment_size,
            ..Default::default()
        },
    )?;

    // Any WAL record with sequence <= this cutoff is already represented by an
    // SSTable recorded in the manifest and must not be replayed.
    let replay_cutoff = max_seq.unwrap_or(0);

    for record in wal.iter(0)? {
        let record = record?;
        let (rec, _) = WalRecord::decode(&record.payload)
            .ok_or_else(|| crate::Error::Corruption("bad wal record during recovery".into()))?;

        max_seq = Some(max_seq.map_or(rec.sequence, |m| m.max(rec.sequence)));

        // Records whose sequence is already reflected in the recovered SSTables
        // (per the manifest's last_sequence) do not need to be replayed.  This
        // prevents duplicate MemTable entries and, for blob values, avoids
        // re-writing large payloads that are already referenced by SSTables.
        if rec.sequence <= replay_cutoff {
            continue;
        }

        if cf_set.get(rec.cf_id).is_none() {
            let _ = cf_set.create_with_id(
                rec.cf_id,
                &format!("recovered_cf_{}", rec.cf_id),
                options.clone(),
            );
        }
        let cf = cf_set.get_mut(rec.cf_id).unwrap();
        match rec.ty {
            WalRecordType::Put => {
                if let Some(value) = rec.value.as_ref() {
                    if options.min_blob_value_size > 0 && value.len() >= options.min_blob_value_size
                    {
                        let blob_ref = blob_store.put(rec.cf_id, &rec.key, value, rec.sequence)?;
                        cf.memtable.lock().unwrap().put_blob_ref(
                            &rec.key,
                            rec.sequence,
                            &blob_ref.encode(),
                        );
                    } else {
                        cf.memtable
                            .lock()
                            .unwrap()
                            .put(&rec.key, rec.sequence, value);
                    }
                } else {
                    cf.memtable.lock().unwrap().put(&rec.key, rec.sequence, &[]);
                }
            }
            WalRecordType::Delete => {
                cf.memtable.lock().unwrap().delete(&rec.key, rec.sequence);
            }
            WalRecordType::DeleteRange => {
                let end = rec.value.as_ref().map(|v| v.as_ref()).unwrap_or(&[]);
                cf.memtable
                    .lock()
                    .unwrap()
                    .delete_range(&rec.key, end, rec.sequence);
            }
        }
    }

    drop(wal);

    Ok(max_seq.unwrap_or(0))
}
