//! Discover procedural macros from crate metadata.

use yelang_proc_macro_bridge::protocol::ProcMacroKind;

use super::{ProcMacroId, ProcMacroRegistry};

/// Stub discovery: in a full build system this would inspect crate metadata and
/// load each proc-macro crate's library through the server.
#[derive(Debug, Default)]
pub struct ProcMacroDiscovery;

impl ProcMacroDiscovery {
    pub fn new() -> Self {
        Self
    }

    /// Discover macros from a static list (used until the build system can
    /// supply proc-macro crate metadata).
    pub fn discover_static(
        &self,
        registry: &mut ProcMacroRegistry,
        entries: &[StaticEntry],
    ) -> Vec<ProcMacroId> {
        let mut ids = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let id = registry.register(
                entry.name.clone(),
                entry.kind,
                // LibraryHandle is slotmap-generated and cannot be constructed
                // without a live session. Static discovery uses placeholder
                // handles that are resolved when the client connects.
                yelang_proc_macro_bridge::protocol::LibraryHandle::default(),
                index as u32,
                entry.path.clone(),
            );
            ids.push(id);
        }
        ids
    }
}

/// A static proc-macro entry for testing and bootstrapping.
#[derive(Debug, Clone)]
pub struct StaticEntry {
    pub name: String,
    pub kind: ProcMacroKind,
    pub path: String,
}
