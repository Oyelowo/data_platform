//! Streaming external k-way merge for compaction.
//!
//! Compaction merges sorted SSTable inputs without loading the full key set into
//! memory.  It deduplicates by user key, drops tombstones only at the deepest
//! level, and splits output files by target size and overlap with the level
//! below the output level.

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;

use crate::FileNumber;
use crate::Result;
use crate::SequenceNumber;
use crate::blob::{BlobRef, BlobStore};
use crate::cache::BlockCaches;
use crate::compaction::Compaction;
use crate::immutable::sstable_path;
use crate::internal_key::{RangeTombstone, ValueType, extract_user_key, parse_internal_key};
use crate::merge_iter::{InternalIterator, MergeIterator};
use crate::metrics::Metrics;
use crate::options::LsmOptions;
use crate::sstable::builder::{SSTableBuilder, SSTableBuilderOptions};
use crate::sstable::reader::SSTableReader;
use crate::version::{FileMetaData, Version};

/// Target output file size for `level`.
pub fn target_file_size(level: usize, options: &LsmOptions) -> u64 {
    assert!(
        level >= 1,
        "target_file_size is only defined for level >= 1"
    );
    let mut size = options.target_file_size_base;
    for _ in 1..level {
        size = size.saturating_mul(options.target_file_size_multiplier);
    }
    size
}

/// Run a streaming compaction merge and return the produced SSTable metadata.
///
/// `output_numbers` must contain enough reserved file numbers for the expected
/// number of output files.  The caller (the engine / compaction worker) is
/// responsible for reserving these numbers before starting the merge so that
/// the merge can run without touching the file-number allocator.
///
/// `smallest_snapshot` is the oldest live snapshot sequence (or the current
/// sequence if there are no snapshots).  Versions newer than it are always
/// kept; the newest version not newer than it is also kept so that the oldest
/// snapshot still sees a value.
#[allow(unused_assignments)]
#[allow(clippy::too_many_arguments)]
pub fn run_compaction_merge(
    db_path: &Path,
    options: &LsmOptions,
    version: &Version,
    job: &Compaction,
    output_numbers: &[FileNumber],
    caches: Option<BlockCaches>,
    metrics: Arc<Metrics>,
    smallest_snapshot: u64,
    blob_store: Option<Arc<BlobStore>>,
) -> Result<(Vec<FileMetaData>, SequenceNumber)> {
    let output_level = job.level + 1;

    let mut children: Vec<Box<dyn InternalIterator>> = Vec::new();
    let mut range_tombstones: Vec<RangeTombstone> = Vec::new();
    let mut max_sequence: SequenceNumber = 0;
    for file in job.inputs.iter().flat_map(|v| v.iter()) {
        let path = sstable_path(db_path, file.number);
        let reader = SSTableReader::open(path, file.number, caches.clone())?;
        for rt in reader.range_tombstones() {
            max_sequence = max_sequence.max(rt.seq);
            range_tombstones.push(rt.clone());
        }
        // Admission policy: compaction input blocks are read exactly once, so
        // they are not admitted into the block caches — caching them would
        // only evict blocks that point lookups keep reusing.
        children.push(Box::new(reader.iter_fill(false)?));
    }

    // Also load range tombstones from all L0 files in the current version.
    // This is a lightweight correctness hedge: a range tombstone in a newer L0
    // file may cover keys in an older file that is being compacted, especially
    // when L0 freezes split a single logical delete_range across MemTables.
    // L0 is small, so reading the meta-blocks is cheap.
    for file in version.levels[0].iter() {
        // Skip files already opened above.
        if job
            .inputs
            .iter()
            .flat_map(|v| v.iter())
            .any(|f| f.number == file.number)
        {
            continue;
        }
        let path = sstable_path(db_path, file.number);
        let reader = SSTableReader::open(path, file.number, caches.clone())?;
        for rt in reader.range_tombstones() {
            max_sequence = max_sequence.max(rt.seq);
            range_tombstones.push(rt.clone());
        }
    }

    let mut merge = MergeIterator::new(children)?;

    let mut outputs: Vec<FileMetaData> = Vec::new();
    let mut current_builder: Option<(FileNumber, SSTableBuilder)> = None;
    let mut current_smallest: Option<Vec<u8>> = None;
    let mut current_largest: Option<Vec<u8>> = None;
    let mut output_idx = 0usize;

    // Sequence number of the most recently emitted entry for the current user
    // key.  Reset to `u64::MAX` whenever the user key changes.
    let mut last_sequence_for_key: u64 = u64::MAX;
    let mut last_user_key: Option<Vec<u8>> = None;

    while merge.valid() {
        let ikey = merge.key().to_vec();
        let value = Bytes::copy_from_slice(merge.value());
        let user_key = extract_user_key(&ikey).to_vec();
        let (seq, ty) = parse_internal_key(&ikey).ok_or_else(|| {
            crate::Error::Sstable("invalid internal key during compaction".into())
        })?;
        // Every input sequence is now represented by this compaction, even if the
        // entry is dropped (hidden by a newer version or covered by a range
        // tombstone).  Tracking all of them keeps the manifest's last_sequence
        // high enough that WAL replay does not re-apply records already folded
        // into SSTables.
        max_sequence = max_sequence.max(seq);

        if last_user_key.as_ref() != Some(&user_key) {
            last_user_key = Some(user_key.clone());
            last_sequence_for_key = u64::MAX;
        }

        // LevelDB rule (A): drop versions hidden by a newer entry for the same
        // user key that is still visible to the oldest snapshot.
        let mut drop = last_sequence_for_key <= smallest_snapshot;

        // Tombstones are dropped only when they reach the deepest level and are
        // old enough that no live snapshot can still observe them.
        if !drop
            && ty == ValueType::Deletion
            && seq <= smallest_snapshot
            && output_level + 1 >= options.num_levels
        {
            drop = true;
        }

        // Range tombstones delete point entries (both values and single-key
        // deletions) that are older than or equal to the tombstone and visible
        // to the oldest snapshot.
        if !drop
            && ty != ValueType::RangeDeletion
            && let Some(t_seq) =
                newest_covering_tombstone(&user_key, seq, &range_tombstones, smallest_snapshot)
            && t_seq >= seq
        {
            drop = true;
        }

        if !drop {
            emit_entry(
                db_path,
                options,
                version,
                output_level,
                output_numbers,
                &mut output_idx,
                &mut outputs,
                &mut current_builder,
                &mut current_smallest,
                &mut current_largest,
                &ikey,
                &value,
                ty,
                seq,
                &range_tombstones,
                smallest_snapshot,
                &metrics,
                blob_store.as_ref(),
                &user_key,
                &mut max_sequence,
            )?;
        }

        last_sequence_for_key = seq;
        merge.next()?;
    }

    if let Some((num, builder)) = current_builder {
        let (meta, file_max_seq) = finish_output(
            builder,
            num,
            &range_tombstones,
            output_level,
            options.num_levels,
            smallest_snapshot,
            current_smallest.as_deref(),
            current_largest.as_deref(),
            &metrics,
        )?;
        outputs.push(meta);
        max_sequence = max_sequence.max(file_max_seq);
    }

    let input_bytes: u64 = job
        .inputs
        .iter()
        .flat_map(|v| v.iter())
        .map(|f| f.file_size)
        .sum();
    let input_files: u64 = job.inputs.iter().map(|v| v.len() as u64).sum();
    metrics.record_compaction(
        input_bytes,
        outputs.iter().map(|f| f.file_size).sum(),
        input_files,
        outputs.len() as u64,
    );

    Ok((outputs, max_sequence))
}

#[allow(clippy::too_many_arguments)]
fn emit_entry(
    db_path: &Path,
    options: &LsmOptions,
    version: &Version,
    output_level: usize,
    output_numbers: &[FileNumber],
    output_idx: &mut usize,
    outputs: &mut Vec<FileMetaData>,
    current_builder: &mut Option<(FileNumber, SSTableBuilder)>,
    current_smallest: &mut Option<Vec<u8>>,
    current_largest: &mut Option<Vec<u8>>,
    ikey: &[u8],
    value: &[u8],
    ty: ValueType,
    seq: SequenceNumber,
    range_tombstones: &[RangeTombstone],
    smallest_snapshot: SequenceNumber,
    metrics: &Metrics,
    blob_store: Option<&Arc<BlobStore>>,
    user_key: &[u8],
    max_sequence: &mut SequenceNumber,
) -> Result<()> {
    if current_builder.is_none() {
        let file_number = output_numbers.get(*output_idx).copied().ok_or_else(|| {
            crate::Error::InvalidArgument(
                "not enough reserved output file numbers for compaction".into(),
            )
        })?;
        let path = sstable_path(db_path, file_number);
        // Bottommost-level blocks are read rarely, so a heavier codec with a
        // better ratio pays off there.  This mirrors the tombstone rule
        // above: the bottommost level is `num_levels - 1`.
        let compression = if output_level + 1 >= options.num_levels {
            options.bottommost_compression
        } else {
            options.compression
        };
        let builder = SSTableBuilder::open(
            path,
            SSTableBuilderOptions {
                block_size: options.block_size,
                block_restart_interval: options.block_restart_interval,
                bloom_bits_per_key: options.bloom_bits_per_key,
                compression,
            },
        )?;
        *current_smallest = Some(ikey.to_vec());
        *current_builder = Some((file_number, builder));
        *output_idx += 1;
    }

    let (_, builder) = current_builder.as_mut().unwrap();

    // Compaction-integrated blob GC: live BlobRefs pointing to non-current blob
    // files are rewritten into the current blob file so the old file can be
    // reclaimed by the standalone GC worker.  The original sequence is preserved
    // so snapshots remain consistent.
    let value = if ty == ValueType::BlobRef {
        maybe_rewrite_blob_value(blob_store, value, user_key, seq)?
    } else {
        value.to_vec()
    };

    builder.add(ikey, &value)?;
    *current_largest = Some(ikey.to_vec());

    // Split the output file if it has grown too large or would overlap too
    // many files in the next-deeper level.
    if should_split_output(
        builder,
        current_smallest.as_ref().unwrap(),
        current_largest.as_ref().unwrap(),
        output_level,
        version,
        options,
    ) {
        let (num, builder) = current_builder.take().unwrap();
        let (meta, file_max_seq) = finish_output(
            builder,
            num,
            range_tombstones,
            output_level,
            options.num_levels,
            smallest_snapshot,
            current_smallest.as_deref(),
            current_largest.as_deref(),
            metrics,
        )?;
        outputs.push(meta);
        *max_sequence = (*max_sequence).max(file_max_seq);
        *current_smallest = None;
        *current_largest = None;
    }

    Ok(())
}

/// If `value` is a `BlobRef` pointing to a non-current blob file, rewrite the
/// blob into the current file and return the encoded new reference.  Otherwise
/// return a copy of the original value.
fn maybe_rewrite_blob_value(
    blob_store: Option<&Arc<BlobStore>>,
    value: &[u8],
    user_key: &[u8],
    seq: SequenceNumber,
) -> Result<Vec<u8>> {
    let Some(blob_store) = blob_store else {
        return Ok(value.to_vec());
    };
    let Some(blob_ref) = BlobRef::decode(value) else {
        return Err(crate::Error::Blob(
            "invalid BlobRef value during compaction".into(),
        ));
    };
    match blob_store.maybe_rewrite_for_compaction(user_key, blob_ref, seq)? {
        Some(new_ref) => Ok(new_ref.encode().to_vec()),
        None => Ok(value.to_vec()),
    }
}

fn should_split_output(
    builder: &SSTableBuilder,
    current_smallest: &[u8],
    current_largest: &[u8],
    output_level: usize,
    version: &Version,
    options: &LsmOptions,
) -> bool {
    if builder.current_size_estimate() >= target_file_size(output_level, options) as usize {
        return true;
    }
    if output_level + 2 >= version.levels.len() {
        return false;
    }
    let overlap = version.overlapping_inputs(output_level + 2, current_smallest, current_largest);
    overlap.len() > options.compaction_max_overlap_files
}

/// Return the sequence number of the newest range tombstone that covers
/// `user_key`, is visible to `smallest_snapshot`, and is at least as new as
/// `point_seq`.
fn newest_covering_tombstone(
    user_key: &[u8],
    point_seq: SequenceNumber,
    range_tombstones: &[RangeTombstone],
    smallest_snapshot: SequenceNumber,
) -> Option<SequenceNumber> {
    let mut best: Option<SequenceNumber> = None;
    for rt in range_tombstones {
        if rt.seq > smallest_snapshot || rt.seq < point_seq {
            continue;
        }
        if rt.covers(user_key) && best.is_none_or(|b| rt.seq > b) {
            best = Some(rt.seq);
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
fn finish_output(
    mut builder: SSTableBuilder,
    number: FileNumber,
    range_tombstones: &[RangeTombstone],
    output_level: usize,
    num_levels: usize,
    smallest_snapshot: SequenceNumber,
    smallest_ikey: Option<&[u8]>,
    largest_ikey: Option<&[u8]>,
    metrics: &Metrics,
) -> Result<(FileMetaData, SequenceNumber)> {
    let is_bottommost = output_level + 1 >= num_levels;
    let mut file_max_seq: SequenceNumber = 0;
    if let (Some(smallest), Some(largest)) = (smallest_ikey, largest_ikey) {
        let smallest_user = extract_user_key(smallest);
        let largest_user = extract_user_key(largest);
        for rt in range_tombstones {
            // Drop range tombstones that have reached the deepest level and are
            // no longer visible to any live snapshot.
            if is_bottommost && rt.seq <= smallest_snapshot {
                continue;
            }
            // Add tombstones whose range overlaps the output file's user-key
            // span.  This keeps tombstones that may cover future point reads
            // against this file.
            if rt.start.as_slice() <= largest_user && rt.end.as_slice() > smallest_user {
                builder.add_range_tombstone(rt.clone())?;
                file_max_seq = file_max_seq.max(rt.seq);
            }
        }
    }

    let built = builder.finish()?;
    metrics.record_compression(built.uncompressed_bytes, built.compressed_bytes);
    Ok((
        FileMetaData {
            number,
            file_size: built.file_size,
            smallest: built.smallest_key,
            largest: built.largest_key,
        },
        file_max_seq,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal_key::{ValueType, build_internal_key};
    use crate::metrics::Metrics;
    use crate::sstable::builder::SSTableBuilderOptions;
    use crate::sstable::reader::SSTableReader;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn ikey(key: &[u8], seq: u64, ty: ValueType) -> Vec<u8> {
        build_internal_key(key, seq, ty)
    }

    fn flush_memtable_to_file(
        dir: &TempDir,
        number: u64,
        entries: &[(Vec<u8>, Vec<u8>)],
    ) -> FileMetaData {
        let path = sstable_path(dir.path(), number);
        let opts = SSTableBuilderOptions::default();
        let mut builder = SSTableBuilder::open(path, opts).unwrap();
        for (k, v) in entries {
            builder.add(k, v).unwrap();
        }
        let built = builder.finish().unwrap();
        FileMetaData {
            number,
            file_size: built.file_size,
            smallest: built.smallest_key,
            largest: built.largest_key,
        }
    }

    fn opts() -> LsmOptions {
        LsmOptions {
            write_buffer_size: 256,
            max_write_buffer_number: 3,
            level0_file_num_compaction_trigger: 4,
            level0_slowdown_writes_trigger: 12,
            level0_stop_writes_trigger: 20,
            max_bytes_for_level_base: 256,
            max_bytes_for_level_multiplier: 10,
            target_file_size_base: 256,
            target_file_size_multiplier: 1,
            num_levels: 7,
            compaction_max_overlap_files: 10,
            block_size: 4 * 1024,
            block_restart_interval: 16,
            bloom_bits_per_key: 10,
            wal_segment_size: 64 * 1024 * 1024,
            block_cache_size: 8 * 1024 * 1024,
            compressed_block_cache_size: 0,
            compression: crate::sstable::format::CompressionType::Lz4,
            bottommost_compression: crate::sstable::format::CompressionType::Zstd,
            logger: None,
            min_blob_value_size: 0,
            blob_file_size: 64 * 1024 * 1024,
            blob_gc_ratio: 0.0,
            blob_gc_interval_ms: 0,
            blob_gc_threads: 1,
            blob_gc_force_threshold: 0.0,
        }
    }

    #[test]
    fn merge_keeps_newest_version_per_key() {
        let dir = TempDir::new().unwrap();
        let options = opts();

        // Two SSTables with overlapping keys; newest sequence wins.
        let entries_a = vec![
            (ikey(b"a", 1, ValueType::Value), b"a-old".to_vec()),
            (ikey(b"b", 3, ValueType::Value), b"b-new".to_vec()),
        ];
        let entries_b = vec![
            (ikey(b"a", 2, ValueType::Value), b"a-new".to_vec()),
            (ikey(b"b", 1, ValueType::Value), b"b-old".to_vec()),
        ];
        let meta_a = flush_memtable_to_file(&dir, 1, &entries_a);
        let meta_b = flush_memtable_to_file(&dir, 2, &entries_b);

        let version = Version::new(7);
        let job = Compaction {
            level: 0,
            inputs: vec![vec![meta_a, meta_b]],
            largest: ikey(b"b", 3, ValueType::Value),
        };

        let (outputs, _max_seq) = run_compaction_merge(
            dir.path(),
            &options,
            &version,
            &job,
            &[3],
            None,
            Arc::new(Metrics::default()),
            3,
            None,
        )
        .unwrap();
        assert_eq!(outputs.len(), 1);

        let reader = SSTableReader::open(
            sstable_path(dir.path(), outputs[0].number),
            outputs[0].number,
            None,
        )
        .unwrap();
        let mut iter = reader.iter().unwrap();
        iter.seek_to_first().unwrap();
        assert!(iter.valid());
        assert_eq!(iter.value(), b"a-new");
        iter.next().unwrap();
        assert!(iter.valid());
        assert_eq!(iter.value(), b"b-new");
        iter.next().unwrap();
        assert!(!iter.valid());
    }

    #[test]
    fn merge_splits_output_by_size() {
        let dir = TempDir::new().unwrap();
        let mut options = opts();
        options.target_file_size_base = 64; // small target

        let mut entries = Vec::new();
        for i in 0u8..20 {
            entries.push((ikey(&[i], 1, ValueType::Value), vec![0u8; 64]));
        }
        let meta = flush_memtable_to_file(&dir, 1, &entries);

        let version = Version::new(7);
        let job = Compaction {
            level: 0,
            inputs: vec![vec![meta]],
            largest: ikey(&[19], 1, ValueType::Value),
        };

        let output_numbers: Vec<u64> = (2..32).collect();
        let (outputs, _max_seq) = run_compaction_merge(
            dir.path(),
            &options,
            &version,
            &job,
            &output_numbers,
            None,
            Arc::new(Metrics::default()),
            1,
            None,
        )
        .unwrap();
        assert!(
            outputs.len() > 1,
            "compaction should have produced more than one output file"
        );
    }

    #[test]
    fn tombstone_dropped_only_at_deepest_level() {
        let dir = TempDir::new().unwrap();
        let mut options = opts();
        options.num_levels = 2; // output level 1 is the deepest

        // Internal-key order stores the newest version first.
        let entries = vec![
            (ikey(b"a", 2, ValueType::Deletion), Vec::new()),
            (ikey(b"a", 1, ValueType::Value), b"v".to_vec()),
        ];
        let meta = flush_memtable_to_file(&dir, 1, &entries);

        let version = Version::new(2);
        let job = Compaction {
            level: 0,
            inputs: vec![vec![meta]],
            largest: ikey(b"a", 2, ValueType::Deletion),
        };

        let (outputs, _max_seq) = run_compaction_merge(
            dir.path(),
            &options,
            &version,
            &job,
            &[2],
            None,
            Arc::new(Metrics::default()),
            2,
            None,
        )
        .unwrap();
        assert_eq!(
            outputs.len(),
            0,
            "tombstone should be dropped at deepest level"
        );
    }

    #[test]
    fn range_tombstone_drops_covered_points_and_survives() {
        use crate::internal_key::RangeTombstone;

        let dir = TempDir::new().unwrap();
        let options = opts();

        let mut entries = Vec::new();
        for i in 0..10u8 {
            entries.push((ikey(&[i], i as u64 + 1, ValueType::Value), vec![i, 1]));
        }
        let mut builder = SSTableBuilder::open(
            sstable_path(dir.path(), 1),
            SSTableBuilderOptions::default(),
        )
        .unwrap();
        for (k, v) in &entries {
            builder.add(k, v).unwrap();
        }
        builder
            .add_range_tombstone(RangeTombstone {
                start: vec![3],
                end: vec![7],
                seq: 100,
            })
            .unwrap();
        let built = builder.finish().unwrap();
        let meta = FileMetaData {
            number: 1,
            file_size: built.file_size,
            smallest: built.smallest_key,
            largest: built.largest_key,
        };

        let version = Version::new(7);
        let job = Compaction {
            level: 0,
            inputs: vec![vec![meta]],
            largest: ikey(&[9], 10, ValueType::Value),
        };

        let (outputs, _max_seq) = run_compaction_merge(
            dir.path(),
            &options,
            &version,
            &job,
            &[2],
            None,
            Arc::new(Metrics::default()),
            100,
            None,
        )
        .unwrap();
        assert_eq!(outputs.len(), 1);

        let mut reader = SSTableReader::open(
            sstable_path(dir.path(), outputs[0].number),
            outputs[0].number,
            None,
        )
        .unwrap();
        for i in 0..10u8 {
            let got = reader.get(&[i], u64::MAX).unwrap();
            if (3..7).contains(&i) {
                assert_eq!(got, Some(None), "key {} should be range-deleted", i);
            } else {
                assert_eq!(got, Some(Some(Bytes::from(vec![i, 1]))));
            }
        }
    }

    #[test]
    fn tombstone_preserved_at_non_deepest_level() {
        let dir = TempDir::new().unwrap();
        let options = opts(); // num_levels = 7, output level 1 is not deepest

        // Internal-key order stores the newest version first.
        let entries = vec![
            (ikey(b"a", 2, ValueType::Deletion), Vec::new()),
            (ikey(b"a", 1, ValueType::Value), b"v".to_vec()),
        ];
        let meta = flush_memtable_to_file(&dir, 1, &entries);

        let version = Version::new(7);
        let job = Compaction {
            level: 0,
            inputs: vec![vec![meta]],
            largest: ikey(b"a", 2, ValueType::Deletion),
        };

        let (outputs, _max_seq) = run_compaction_merge(
            dir.path(),
            &options,
            &version,
            &job,
            &[2],
            None,
            Arc::new(Metrics::default()),
            2,
            None,
        )
        .unwrap();
        assert_eq!(
            outputs.len(),
            1,
            "tombstone should survive until deepest level"
        );
    }
}
