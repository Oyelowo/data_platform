//! Exchange operators and flow control for distributed execution.

use crate::errors::ExecError;
use crate::exec::value::RecordBatch;
use crate::pir::operator::ExchangeKind;

/// Send a batch to the appropriate endpoint for an exchange.
pub fn route_batch(
    batch: RecordBatch,
    kind: &ExchangeKind,
    _partition_count: usize,
) -> Result<Vec<RecordBatch>, ExecError> {
    match kind {
        ExchangeKind::Single => Ok(vec![batch]),
        ExchangeKind::Gather => Ok(vec![batch]),
        ExchangeKind::Broadcast => Ok(vec![batch]),
        ExchangeKind::RepartitionBy(_) => {
            // TODO: hash-partition batch.
            Ok(vec![batch])
        }
        ExchangeKind::RangePartition(_) => {
            // TODO: range-partition batch.
            Ok(vec![batch])
        }
    }
}
