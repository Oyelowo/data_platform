//! Resolve macro names to procedural macro definitions.

use super::{ProcMacroDef, ProcMacroRegistry};

/// Resolver for procedural macro names.
#[derive(Debug, Default)]
pub struct ProcMacroResolver {
    registry: ProcMacroRegistry,
}

impl ProcMacroResolver {
    pub fn new(registry: ProcMacroRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ProcMacroRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut ProcMacroRegistry {
        &mut self.registry
    }

    pub fn resolve(&self, name: &str) -> Option<&ProcMacroDef> {
        self.registry.find_by_name(name)
    }
}
