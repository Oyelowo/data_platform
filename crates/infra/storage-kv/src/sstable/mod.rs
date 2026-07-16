//! SSTable building and reading.

pub mod block;
pub mod builder;
pub mod filter;
pub mod format;
pub mod index;
pub mod reader;

pub use builder::{SSTableBuilder, SSTableBuilderOptions};
pub use reader::{SSTableReader, SSTableIterator};

/// The maximum user key size supported by the SSTable format.
pub const MAX_KEY_SIZE: usize = 16 * 1024 * 1024;

/// The maximum inline value size supported by the SSTable format.
pub const MAX_VALUE_SIZE: usize = 64 * 1024 * 1024;
