//! Discover procedural macros from proc-macro crate sources.
//!
//! Two discovery paths exist (see the Phase 7.5 design doc):
//!
//! - **Manifest discovery** (`*.ypm.json` sidecar): reads and validates crate
//!   metadata without loading any code, mirroring how rustc reads
//!   `proc_macro_data` from crate metadata. Preferred; performs no `dlopen`.
//! - **Introspection fallback**: loads the dylib through the proc-macro
//!   server and registers the descriptors the server reports (mirroring
//!   rust-analyzer's `ListMacros`). Used when no manifest exists. The load
//!   happens inside the server process and the returned handle is cached, so
//!   it never costs a second load at expansion time.
//!
//! In both paths the `macro_index` of a registered macro is its position in
//! the dylib's export order — reported by the server for introspection, and
//! cross-checked against the dylib at first load for manifests.
//!
//! Invariant: each canonical dylib path may be registered at most once. Two
//! manifests for the same dylib (even with different crate names) are
//! rejected, because the macro indices and descriptor cross-check are per
//! dylib, not per crate name.

use std::path::{Path, PathBuf};

use thiserror::Error;
use yelang_proc_macro_bridge::protocol::ProcMacroKind;

use super::client::{LoadedLibrary, ProcMacroClientError};
use super::manifest::{ProcMacroCrateManifest, crate_name_from_dylib_path, sidecar_manifest_path};
use super::registry::{ProcMacroId, ProcMacroRegistry};

/// A source of proc-macro crates handed to discovery.
#[derive(Debug, Clone)]
pub enum ProcMacroSource {
    /// Explicit path to a `*.ypm.json` manifest. No probing, no `dlopen`.
    Manifest(PathBuf),
    /// Path to a compiled dylib. If a `<dylib file stem>.ypm.json` sidecar
    /// exists next to it, the manifest path is used; otherwise the dylib is
    /// introspected through the server.
    Dylib(PathBuf),
}

/// Errors discovery can report. A failed source leaves the registry
/// untouched (all-or-nothing per source).
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("IO error while {op} at {path}: {message}")]
    Io {
        op: &'static str,
        path: PathBuf,
        message: String,
    },
    #[error("manifest at {path} is {size} bytes (max {MAX} bytes)", MAX = super::manifest::MAX_MANIFEST_BYTES)]
    ManifestTooLarge { path: PathBuf, size: u64 },
    #[error("failed to parse manifest at {path}: {message}")]
    ManifestParse { path: PathBuf, message: String },
    #[error("manifest at {path} has format version {found}, expected {expected}")]
    UnsupportedFormatVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    #[error("invalid manifest at {path}: {reason}")]
    InvalidManifest { path: PathBuf, reason: String },
    #[error(
        "proc-macro crate {crate_name} speaks protocol version {found}, compiler expects {expected}"
    )]
    ProtocolMismatch {
        crate_name: String,
        found: u32,
        expected: u32,
    },
    #[error("proc-macro crate {crate_name} was built for {manifest}, but the host is {host}")]
    TripleMismatch {
        crate_name: String,
        manifest: String,
        host: String,
    },
    #[error("dylib at {path} is {found} bytes, manifest says {expected}")]
    SizeMismatch {
        path: PathBuf,
        expected: u64,
        found: u64,
    },
    #[error(
        "dylib at {path} does not match the content hash in its manifest (stale or tampered artifact)"
    )]
    HashMismatch { path: PathBuf },
    #[error("proc-macro crate {crate_name} exports no macros")]
    EmptyLibrary { crate_name: String },
    #[error(
        "duplicate {kind:?} proc macro `{name}`: exported by both `{first_crate}` and `{second_crate}`"
    )]
    DuplicateMacro {
        name: String,
        kind: ProcMacroKind,
        first_crate: String,
        second_crate: String,
    },
    #[error(
        "dylib at {path} is already registered as proc-macro crate `{first_crate}`; \
         cannot register it again as `{second_crate}`"
    )]
    DuplicateLibrary {
        path: PathBuf,
        first_crate: String,
        second_crate: String,
    },
    #[error("failed to introspect dylib: {0}")]
    Introspection(ProcMacroClientError),
}

/// How a discovered crate's metadata was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Read from a manifest; the dylib was not loaded.
    Manifest,
    /// Reported by the server after loading the dylib.
    Introspected,
}

/// The outcome of discovering one source.
#[derive(Debug)]
pub struct DiscoveredCrate {
    pub crate_name: String,
    pub provenance: Provenance,
    pub macro_ids: Vec<ProcMacroId>,
}

/// Aggregate result of [`ProcMacroRuntime::discover_all`]: every source is
/// attempted, errors do not stop later sources.
#[derive(Debug, Default)]
pub struct DiscoveryReport {
    pub discovered: Vec<DiscoveredCrate>,
    pub errors: Vec<DiscoveryError>,
}

impl DiscoveryReport {
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Register every macro exported by `source` into `registry`.
///
/// `load` performs a (caching) server load of a canonical dylib path; it is
/// only invoked for the introspection fallback. The second argument is
/// `pre_validated`: if true, the cached entry is marked already validated
/// (used for introspection, where the macro indices come directly from the
/// descriptors). Manifest discovery never loads code.
pub(crate) fn discover_source(
    registry: &mut ProcMacroRegistry,
    load: &mut dyn FnMut(&Path, bool) -> Result<LoadedLibrary, ProcMacroClientError>,
    source: &ProcMacroSource,
) -> Result<DiscoveredCrate, DiscoveryError> {
    match source {
        ProcMacroSource::Manifest(path) => discover_manifest(registry, path),
        ProcMacroSource::Dylib(path) => {
            let sidecar = sidecar_manifest_path(path);
            if sidecar.exists() {
                discover_manifest(registry, &sidecar)
            } else {
                discover_introspected(registry, load, path)
            }
        }
    }
}

fn discover_manifest(
    registry: &mut ProcMacroRegistry,
    path: &Path,
) -> Result<DiscoveredCrate, DiscoveryError> {
    let manifest = ProcMacroCrateManifest::read(path)?;
    let canonical_dylib = manifest.validate_environment(path)?;
    let macros = manifest
        .macros
        .iter()
        .map(|m| (m.name.clone(), m.kind))
        .collect::<Vec<_>>();
    register_all(
        registry,
        &manifest.crate_name,
        &canonical_dylib,
        macros,
        Provenance::Manifest,
    )
}

fn discover_introspected(
    registry: &mut ProcMacroRegistry,
    load: &mut dyn FnMut(&Path, bool) -> Result<LoadedLibrary, ProcMacroClientError>,
    path: &Path,
) -> Result<DiscoveredCrate, DiscoveryError> {
    let canonical = std::fs::canonicalize(path).map_err(|e| DiscoveryError::Io {
        op: "canonicalize dylib",
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let loaded = load(&canonical, true).map_err(DiscoveryError::Introspection)?;
    let crate_name = crate_name_from_dylib_path(path);
    if loaded.descriptors.is_empty() {
        return Err(DiscoveryError::EmptyLibrary { crate_name });
    }
    let macros = loaded
        .descriptors
        .iter()
        .map(|d| (d.name.clone(), d.kind))
        .collect::<Vec<_>>();
    register_all(
        registry,
        &crate_name,
        &canonical,
        macros,
        Provenance::Introspected,
    )
}

/// Register `macros` (in dylib export order) after a whole-batch duplicate
/// check, so a failed source leaves the registry untouched.
fn register_all(
    registry: &mut ProcMacroRegistry,
    crate_name: &str,
    canonical_dylib: &Path,
    macros: Vec<(String, ProcMacroKind)>,
    provenance: Provenance,
) -> Result<DiscoveredCrate, DiscoveryError> {
    let library_path = canonical_dylib.to_string_lossy().into_owned();

    // Enforce the one-dylib-per-registration invariant.
    if let Some(existing) = registry.iter().find(|m| m.library_path == library_path) {
        if existing.crate_name != crate_name {
            return Err(DiscoveryError::DuplicateLibrary {
                path: canonical_dylib.to_path_buf(),
                first_crate: existing.crate_name.clone(),
                second_crate: crate_name.to_string(),
            });
        }
    }

    for (name, kind) in &macros {
        if let Some(existing) = registry.find(name, *kind) {
            return Err(DiscoveryError::DuplicateMacro {
                name: name.clone(),
                kind: *kind,
                first_crate: existing.crate_name.clone(),
                second_crate: crate_name.to_string(),
            });
        }
    }

    let macro_ids = macros
        .into_iter()
        .enumerate()
        .map(|(index, (name, kind))| {
            registry.register(
                name,
                kind,
                index as u32,
                library_path.clone(),
                crate_name.to_string(),
            )
        })
        .collect();

    Ok(DiscoveredCrate {
        crate_name: crate_name.to_string(),
        provenance,
        macro_ids,
    })
}
