//! Registry of known procedural macros.

use yelang_proc_macro_bridge::protocol::ProcMacroKind;

/// A handle for a registered procedural macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcMacroId(pub usize);

/// A registered procedural macro.
///
/// `ProcMacroDef` stores metadata only. The runtime-assigned `LibraryHandle` is
/// managed by `ProcMacroRuntime` and resolved lazily when the macro is first
/// invoked.
#[derive(Debug, Clone)]
pub struct ProcMacroDef {
    pub id: ProcMacroId,
    pub name: String,
    pub kind: ProcMacroKind,
    pub macro_index: u32,
    pub library_path: String,
    /// Name of the proc-macro crate that exports this macro.
    pub crate_name: String,
    /// Optional span pointing to the macro definition in the proc-macro crate.
    /// When known, this is sent to the proc-macro API as `Span::def_site()`.
    pub def_site_span: Option<yelang_lexer::Span>,
}

/// Registry of all procedural macros available to the current crate.
///
/// Lookup is kind-aware, matching the separate macro namespaces (function-like,
/// attribute, derive): macros of different kinds may share a name, while a
/// duplicate `(name, kind)` pair is rejected at discovery time.
#[derive(Debug, Default)]
pub struct ProcMacroRegistry {
    macros: Vec<ProcMacroDef>,
}

impl ProcMacroRegistry {
    pub fn new() -> Self {
        Self { macros: Vec::new() }
    }

    /// Register a new macro. The `library_path` is canonicalized if the file
    /// exists, with a fallback to the raw path; callers should prefer passing
    /// a canonical path to avoid cache-key mismatches.
    pub fn register(
        &mut self,
        name: String,
        kind: ProcMacroKind,
        macro_index: u32,
        library_path: String,
        crate_name: String,
        def_site_span: Option<yelang_lexer::Span>,
    ) -> ProcMacroId {
        let canonical_path = std::fs::canonicalize(&library_path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(library_path);
        let id = ProcMacroId(self.macros.len());
        self.macros.push(ProcMacroDef {
            id,
            name,
            kind,
            macro_index,
            library_path: canonical_path,
            crate_name,
            def_site_span,
        });
        id
    }

    pub fn get(&self, id: ProcMacroId) -> Option<&ProcMacroDef> {
        self.macros.get(id.0)
    }

    /// Kind-aware lookup: macros live in separate namespaces per kind.
    pub fn find(&self, name: &str, kind: ProcMacroKind) -> Option<&ProcMacroDef> {
        self.macros
            .iter()
            .find(|m| m.name == name && m.kind == kind)
    }

    /// The `(name, kind)` sequence the dylib at `library_path` is expected to
    /// export, ordered by `macro_index`. Used by the runtime's load-time
    /// descriptor cross-check.
    pub fn expected_descriptors(&self, library_path: &str) -> Vec<(String, ProcMacroKind)> {
        let mut entries: Vec<&ProcMacroDef> = self
            .macros
            .iter()
            .filter(|m| m.library_path == library_path)
            .collect();
        entries.sort_by_key(|m| m.macro_index);
        entries
            .into_iter()
            .map(|m| (m.name.clone(), m.kind))
            .collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProcMacroDef> {
        self.macros.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ProcMacroRegistry {
        let mut registry = ProcMacroRegistry::new();
        registry.register(
            "make_answer".to_string(),
            ProcMacroKind::FunctionLike,
            0,
            "/lib/a.dylib".to_string(),
            "crate_a".to_string(),
            None,
        );
        registry.register(
            "trace".to_string(),
            ProcMacroKind::Attribute,
            1,
            "/lib/a.dylib".to_string(),
            "crate_a".to_string(),
            None,
        );
        registry.register(
            "make_answer".to_string(),
            ProcMacroKind::Derive,
            0,
            "/lib/b.dylib".to_string(),
            "crate_b".to_string(),
            None,
        );
        registry
    }

    #[test]
    fn find_is_kind_aware() {
        let registry = registry();
        assert_eq!(
            registry
                .find("make_answer", ProcMacroKind::FunctionLike)
                .unwrap()
                .crate_name,
            "crate_a"
        );
        assert_eq!(
            registry
                .find("make_answer", ProcMacroKind::Derive)
                .unwrap()
                .crate_name,
            "crate_b"
        );
        assert!(
            registry
                .find("make_answer", ProcMacroKind::Attribute)
                .is_none()
        );
        assert!(registry.find("nope", ProcMacroKind::FunctionLike).is_none());
    }

    #[test]
    fn expected_descriptors_are_ordered_by_macro_index() {
        let mut registry = registry();
        // Register out of order on purpose.
        registry.register(
            "z_last".to_string(),
            ProcMacroKind::FunctionLike,
            3,
            "/lib/c.dylib".to_string(),
            "crate_c".to_string(),
            None,
        );
        registry.register(
            "a_first".to_string(),
            ProcMacroKind::FunctionLike,
            0,
            "/lib/c.dylib".to_string(),
            "crate_c".to_string(),
            None,
        );
        let expected = registry.expected_descriptors("/lib/c.dylib");
        assert_eq!(
            expected,
            vec![
                ("a_first".to_string(), ProcMacroKind::FunctionLike),
                ("z_last".to_string(), ProcMacroKind::FunctionLike),
            ]
        );
        assert!(registry.expected_descriptors("/lib/none.dylib").is_empty());
    }
}
