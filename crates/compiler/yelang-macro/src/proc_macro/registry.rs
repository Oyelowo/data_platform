//! Registry of known procedural macros.

use yelang_proc_macro_bridge::protocol::{LibraryHandle, ProcMacroKind};

/// A handle for a registered procedural macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcMacroId(pub usize);

/// A registered procedural macro.
#[derive(Debug, Clone)]
pub struct ProcMacroDef {
    pub id: ProcMacroId,
    pub name: String,
    pub kind: ProcMacroKind,
    pub library: LibraryHandle,
    pub macro_index: u32,
    pub library_path: String,
}

/// Registry of all procedural macros available to the current crate.
#[derive(Debug, Default)]
pub struct ProcMacroRegistry {
    macros: Vec<ProcMacroDef>,
}

impl ProcMacroRegistry {
    pub fn new() -> Self {
        Self { macros: Vec::new() }
    }

    pub fn register(
        &mut self,
        name: String,
        kind: ProcMacroKind,
        library: LibraryHandle,
        macro_index: u32,
        library_path: String,
    ) -> ProcMacroId {
        let id = ProcMacroId(self.macros.len());
        self.macros.push(ProcMacroDef {
            id,
            name,
            kind,
            library,
            macro_index,
            library_path,
        });
        id
    }

    pub fn get(&self, id: ProcMacroId) -> Option<&ProcMacroDef> {
        self.macros.get(id.0)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&ProcMacroDef> {
        self.macros.iter().find(|m| m.name == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProcMacroDef> {
        self.macros.iter()
    }
}
