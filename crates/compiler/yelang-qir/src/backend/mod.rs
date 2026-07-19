//! Pluggable storage backend capability model.

pub mod capability;
pub mod memory_backend;
pub mod remote_backend;

pub use capability::{BackendCapability, Cardinality};
pub use memory_backend::MemoryBackend;
pub use remote_backend::RemoteBackend;
