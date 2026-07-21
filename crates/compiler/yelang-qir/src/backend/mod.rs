//! Pluggable storage backend capability model.

pub mod memory;
pub mod remote;
pub mod storage_backend;
pub mod stream_backend;

pub use crate::pir::capability::{BackendCapability, Cardinality, Support, supports_aggregate_op};
pub use memory::MemoryBackend;
pub use remote::RemoteBackend;
pub use storage_backend::StorageBackend;
pub use stream_backend::StreamBackend;
