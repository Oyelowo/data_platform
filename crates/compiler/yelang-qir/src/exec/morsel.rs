//! Morsel-driven scheduler for parallel query execution.

use crate::exec::value::RecordBatch;

/// A small chunk of data processed by one pipeline thread.
#[derive(Clone, Debug, Default)]
pub struct Morsel {
    pub batch: RecordBatch,
    pub pipeline_id: u32,
    pub morsel_id: u64,
}

/// A sink that consumes morsels.
pub trait MorselSink: Send {
    fn push(&mut self, morsel: Morsel);
    fn finish(self: Box<Self>) -> Vec<RecordBatch>;
}

/// A source that produces morsels.
pub trait MorselSource: Send {
    fn next(&mut self) -> Option<Morsel>;
}
