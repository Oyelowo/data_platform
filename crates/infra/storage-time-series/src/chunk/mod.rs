//! Immutable, compressed time-series chunks.

pub mod builder;
pub mod encoding;
pub mod reader;

pub use builder::ChunkBuilder;
pub use reader::ChunkReader;
