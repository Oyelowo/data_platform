//! Segment-based inverted index.

pub mod memory;
pub mod merger;
pub mod reader;
pub mod segment;
pub mod writer;

pub use memory::MemorySegment;
pub use merger::merge_segments;
pub use reader::SegmentReader;
pub use segment::{ImmutableSegment, SegmentData};
pub use writer::SegmentWriter;

/// File name for the combined segment payload.
pub const SEGMENT_FILE: &str = "segment.bin";
