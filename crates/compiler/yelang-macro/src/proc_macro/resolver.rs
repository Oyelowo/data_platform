//! Resolve macro names to procedural macro definitions and manage the runtime
//! connection to the proc-macro server.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use yelang_proc_macro_bridge::protocol::{LibraryHandle, ProcMacroKind};

use super::client::{LoadedLibrary, ProcMacroClient, ProcMacroClientError};
use super::discovery::{
    DiscoveredCrate, DiscoveryError, DiscoveryReport, ProcMacroSource, discover_source,
};
use super::registry::{ProcMacroDef, ProcMacroRegistry};
use crate::error::ExpandError;

/// A procedural macro that has been resolved and whose library is loaded.
#[derive(Debug, Clone)]
pub struct ResolvedProcMacro {
    pub name: String,
    pub kind: ProcMacroKind,
    pub library: LibraryHandle,
    pub macro_index: u32,
}

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

    /// Resolve a name in the namespace of `kind` (function-like, attribute,
    /// and derive macros are separate namespaces).
    pub fn resolve(&self, name: &str, kind: ProcMacroKind) -> Option<&ProcMacroDef> {
        self.registry.find(name, kind)
    }
}

/// Runtime state for out-of-process procedural macros.
///
/// Owns the server connection and lazily loads proc-macro libraries on first
/// use. Load results are cached per canonical path so a library is loaded at
/// most once per session, and failures are reported once.
pub struct ProcMacroRuntime {
    client: RefCell<ProcMacroClient>,
    resolver: ProcMacroResolver,
    loaded: RefCell<HashMap<PathBuf, Result<LoadedLibrary, ProcMacroClientError>>>,
}

impl std::fmt::Debug for ProcMacroRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcMacroRuntime")
            .field("resolver", &self.resolver)
            .field("loaded_count", &self.loaded.borrow().len())
            .finish_non_exhaustive()
    }
}

impl ProcMacroRuntime {
    /// Create a runtime from a connected client and a resolver.
    pub fn new(client: ProcMacroClient, resolver: ProcMacroResolver) -> Self {
        Self {
            client: RefCell::new(client),
            resolver,
            loaded: RefCell::new(HashMap::new()),
        }
    }

    /// Discover and register the macros exported by one proc-macro crate
    /// source. See [`ProcMacroSource`] for the manifest vs. introspection
    /// paths. A failed discovery leaves the registry untouched.
    pub fn discover(
        &mut self,
        source: &ProcMacroSource,
    ) -> Result<DiscoveredCrate, DiscoveryError> {
        // Split borrows: the loader needs the client and the cache, discovery
        // needs the registry.
        let Self {
            client,
            resolver,
            loaded,
        } = self;
        let mut load = |path: &Path| load_cached(client, loaded, path);
        discover_source(resolver.registry_mut(), &mut load, source)
    }

    /// Discover several sources, continuing past individual failures so a
    /// driver sees every bad extern in one pass.
    pub fn discover_all(&mut self, sources: &[ProcMacroSource]) -> DiscoveryReport {
        let mut report = DiscoveryReport::default();
        for source in sources {
            match self.discover(source) {
                Ok(discovered) => report.discovered.push(discovered),
                Err(error) => report.errors.push(error),
            }
        }
        report
    }

    /// Resolve a macro name in the namespace of `kind` to a loaded macro.
    ///
    /// Returns `None` if the name is not a known proc macro of that kind.
    /// Returns `Some(Err(...))` if the macro is known but its library could
    /// not be loaded or failed validation.
    pub fn resolve(
        &self,
        name: &str,
        kind: ProcMacroKind,
    ) -> Option<Result<ResolvedProcMacro, ProcMacroClientError>> {
        let def = self.resolver.resolve(name, kind)?;
        Some(
            self.load_validated(Path::new(&def.library_path))
                .map(|library| ResolvedProcMacro {
                    name: def.name.clone(),
                    kind: def.kind,
                    library,
                    macro_index: def.macro_index,
                }),
        )
    }

    /// The resolver holding the discovery-populated registry.
    pub fn resolver(&self) -> &ProcMacroResolver {
        &self.resolver
    }

    /// Number of libraries currently held in the load cache (loaded or
    /// failed). Exposed for drivers and tests: manifest-based discovery must
    /// leave this at zero until the first expansion.
    pub fn loaded_library_count(&self) -> usize {
        self.loaded.borrow().len()
    }

    /// Expand a server-based procedural macro.
    pub fn expand_proc_macro(
        &self,
        macro_def: &ResolvedProcMacro,
        args: Option<yelang_proc_macro_bridge::protocol::WireTokenStream>,
        item: Option<yelang_proc_macro_bridge::protocol::WireTokenStream>,
        span: yelang_lexer::Span,
    ) -> Result<
        (
            yelang_proc_macro_bridge::protocol::WireTokenStream,
            Vec<yelang_proc_macro_bridge::protocol::token::WireDiagnostic>,
        ),
        ExpandError,
    > {
        crate::proc_macro::expand_proc_macro(
            &mut *self.client.borrow_mut(),
            macro_def,
            args,
            item,
            span,
        )
    }

    /// Load a library for expansion: caching, and on a fresh load the
    /// server's descriptors are cross-checked against the registry's
    /// expectation for this path (catching manifest/dylib skew). A mismatch
    /// replaces the cached entry with the validation error so it is reported
    /// once, consistently.
    fn load_validated(&self, path: &Path) -> Result<LibraryHandle, ProcMacroClientError> {
        let key = canonical_key(path);
        if let Some(result) = self.loaded.borrow().get(&key) {
            return result.clone().map(|loaded| loaded.handle);
        }

        let loaded = load_cached(&self.client, &self.loaded, &key)?;

        let expected = self
            .resolver
            .registry()
            .expected_descriptors(&key.to_string_lossy());
        let found: Vec<(String, ProcMacroKind)> = loaded
            .descriptors
            .iter()
            .map(|d| (d.name.clone(), d.kind))
            .collect();
        if expected != found {
            let error = ProcMacroClientError::Validation(format!(
                "expected macros {expected:?}, but the dylib exports {found:?}"
            ));
            self.loaded.borrow_mut().insert(key, Err(error.clone()));
            return Err(error);
        }
        Ok(loaded.handle)
    }
}

/// Canonicalize a path for use as a cache key; falls back to the raw path if
/// canonicalization fails (the server will report the IO error on load).
fn canonical_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Load a library through the client, caching the outcome per canonical path
/// so a given library is loaded at most once per session.
fn load_cached(
    client: &RefCell<ProcMacroClient>,
    loaded: &RefCell<HashMap<PathBuf, Result<LoadedLibrary, ProcMacroClientError>>>,
    path: &Path,
) -> Result<LoadedLibrary, ProcMacroClientError> {
    let key = canonical_key(path);
    if let Some(result) = loaded.borrow().get(&key) {
        return result.clone();
    }

    let result = client.borrow_mut().load_library(&key.to_string_lossy());
    loaded.borrow_mut().insert(key, result.clone());
    result
}
