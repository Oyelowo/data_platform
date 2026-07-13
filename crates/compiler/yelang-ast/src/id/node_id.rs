use std::sync::atomic::{AtomicU32, Ordering};

/// Unique ID for an AST node
///
/// Assigned during parsing. Unique across the entire program
/// (all files, all modules). Used as key in resolution maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Dummy NodeId for testing or error recovery
    pub const DUMMY: Self = NodeId(u32::MAX);

    /// Create a new unique NodeId
    pub fn new() -> Self {
        static NEXT_ID: AtomicU32 = AtomicU32::new(0);
        NodeId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Create from raw u32 (for deserialization)
    pub fn from_u32(id: u32) -> Self {
        NodeId(id)
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}
