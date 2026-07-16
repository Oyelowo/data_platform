//! MemTable-to-SSTable flush logic.

use std::path::Path;
use std::sync::{Arc, Mutex};

use bytes::Bytes;

use crate::immutable::sstable_path;
use crate::internal_key::extract_user_key;
use crate::manifest::Manifest;
use crate::memtable::MemTable;
use crate::metrics::Metrics;
use crate::options::LsmOptions;
use crate::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};
use crate::version::FileMetaData;
use crate::version_set::{VersionEdit, VersionSet};
use crate::{Result, SequenceNumber};

/// Flush a MemTable to a new L0 SSTable and update the VersionSet.
///
/// `file_number` must be the number reserved when the MemTable was frozen
/// (see [`crate::immutable::ImmutableMemTables`]); reserving at freeze time is
/// what keeps L0 file-number order identical to version order.
#[allow(clippy::too_many_arguments)]
pub fn flush_memtable(
    db_path: &Path,
    options: &LsmOptions,
    version_set: &Arc<VersionSet>,
    manifest: &Arc<Mutex<Manifest>>,
    mem: &MemTable,
    last_sequence: SequenceNumber,
    file_number: crate::FileNumber,
    metrics: &Metrics,
    cf_id: crate::column_family::ColumnFamilyId,
) -> Result<FileMetaData> {
    let path = sstable_path(db_path, file_number);
    let opts = SSTableBuilderOptions {
        block_size: options.block_size,
        block_restart_interval: options.block_restart_interval,
        bloom_bits_per_key: options.bloom_bits_per_key,
        // Flush always produces L0 files, which are never bottommost.
        compression: options.compression,
    };
    let mut builder = SSTableBuilder::open(path, opts)?;

    // The MemTable iter is sorted by internal-key comparator (user key
    // ascending, sequence descending). Keep only the newest version of each
    // user key, then reorder by raw internal-key bytes because the SSTable
    // block format relies on lexicographic ordering.
    let mut deduped: Vec<(Vec<u8>, Bytes)> = Vec::new();
    let mut last_user_key: Option<Vec<u8>> = None;
    for (ikey, value) in mem.iter() {
        let user_key = extract_user_key(&ikey).to_vec();
        if Some(&user_key) == last_user_key.as_ref() {
            continue;
        }
        last_user_key = Some(user_key);
        deduped.push((ikey, value));
    }
    deduped.sort_by(|a, b| a.0.cmp(&b.0));
    for (ikey, value) in deduped {
        builder.add(&ikey, &value)?;
    }

    // Range tombstones are stored in a dedicated meta-block so they can be
    // loaded once per SSTable open and applied to both point reads and scans.
    for rt in mem.range_tombstones() {
        builder.add_range_tombstone(rt)?;
    }

    let built = builder.finish()?;
    metrics.record_compression(built.uncompressed_bytes, built.compressed_bytes);

    let meta = FileMetaData {
        number: file_number,
        file_size: built.file_size,
        smallest: built.smallest_key,
        largest: built.largest_key,
    };

    let edit = VersionEdit {
        cf_id,
        new_files: vec![(0, meta.clone())],
        last_sequence,
        next_file_number: version_set.next_file_number(),
        ..Default::default()
    };
    manifest.lock().unwrap().log_edit(&edit)?;
    version_set.apply(edit)?;
    Ok(meta)
}
