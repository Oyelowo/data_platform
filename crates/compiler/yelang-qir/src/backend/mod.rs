//! Pluggable storage backend capability model.

pub mod capability;
pub mod memory_backend;
pub mod remote_backend;
pub mod storage_backend;
pub mod stream_backend;

pub use capability::{BackendCapability, Cardinality, Support, supports_aggregate_op};
pub use memory_backend::MemoryBackend;
pub use remote_backend::RemoteBackend;
pub use storage_backend::StorageBackend;
pub use stream_backend::StreamBackend;
