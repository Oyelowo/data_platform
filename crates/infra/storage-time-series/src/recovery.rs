//! Crash recovery: replay WAL and rebuild the label index.

use std::collections::HashSet;

use crate::format::{Metadata, Sample, WalRecord};
use crate::memtable::MemTable;
use crate::options::TimeSeriesOptions;
use crate::wal::TimeSeriesWal;

/// Replay all WAL records into the memtable and update metadata.
pub fn replay_wal(
    wal: &TimeSeriesWal,
    memtable: &mut MemTable,
    metadata: &mut Metadata,
    options: &TimeSeriesOptions,
) -> crate::Result<()> {
    for record in wal.iter()? {
        let record = record?;
        match record {
            WalRecord::Put {
                series_key,
                timestamp,
                value,
            } => {
                metadata.series.insert(series_key.clone());
                metadata.label_index.insert(series_key.clone())?;
                memtable.insert(series_key, Sample { timestamp, value });
            }
            WalRecord::DeleteSeries { series_key } => {
                metadata.series.remove(&series_key);
                metadata.label_index.remove(&series_key);
                memtable.delete_series(&series_key);
            }
            WalRecord::DeleteRange {
                series_key,
                start,
                end,
            } => {
                memtable.delete_range(&series_key, start, end);
            }
            WalRecord::Checkpoint { metadata: cp } => {
                // Checkpoint metadata is advisory; validate options match.
                if cp.options != *options {
                    return Err(crate::Error::invalid_argument(
                        "checkpoint options do not match engine options",
                    ));
                }
                metadata.wal_checkpoint_lsn =
                    metadata.wal_checkpoint_lsn.max(cp.wal_checkpoint_lsn);
            }
        }
    }
    Ok(())
}

/// Rebuild the series set and label index from chunk files if the persisted
/// index is missing or corrupt.
pub fn rebuild_index_from_chunks(
    series: &mut HashSet<Vec<u8>>,
    index: &mut crate::index::LabelIndex,
    chunk_series_keys: &[Vec<u8>],
) -> crate::Result<()> {
    for key in chunk_series_keys {
        series.insert(key.clone());
        index.insert(key.clone())?;
    }
    Ok(())
}
