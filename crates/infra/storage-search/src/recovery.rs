//! WAL replay for the search engine.

use crate::format::{Metadata, WalRecord};
use crate::index::memory::MemorySegment;
use crate::wal::SearchWal;

/// Replay WAL records into the memory segment and update metadata.
pub fn replay(
    wal: &SearchWal,
    memory: &mut MemorySegment,
    metadata: &mut Metadata,
) -> crate::Result<()> {
    for rec in wal.iter()? {
        match rec? {
            WalRecord::IndexDocument { doc_id, document } => {
                memory.index_document(doc_id, &document, &metadata.schema)?;
            }
            WalRecord::DeleteDocument { doc_id } => {
                memory.delete_document(&doc_id);
            }
            WalRecord::Checkpoint { metadata: chk } => {
                *metadata = chk;
            }
        }
    }
    Ok(())
}
