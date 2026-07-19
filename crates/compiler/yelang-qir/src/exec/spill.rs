//! Radix-partitioned spilling for operators that exceed memory.

use crate::errors::ExecError;
use crate::exec::value::RecordBatch;

/// A spill file handle.
#[derive(Debug)]
pub struct SpillFile {
    pub id: u64,
    pub batches: Vec<RecordBatch>,
}

/// Spill batches to temporary storage.
pub fn spill_batches(
    batches: Vec<RecordBatch>,
    _radix_bits: u8,
) -> Result<Vec<SpillFile>, ExecError> {
    // TODO: implement radix partitioning and file I/O.
    Ok(vec![SpillFile { id: 0, batches }])
}
