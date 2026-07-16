//! Procedural macro integration.
//!
//! This module discovers procedural macros from proc-macro crate manifests or
//! by server introspection, communicates with the yelang-proc-macro-server,
//! and integrates expansion results back into the declarative macro expander.

pub mod client;
pub mod discovery;
pub mod executor;
pub mod expand;
pub mod export;
pub mod manifest;
pub mod registry;
pub mod resolver;

pub use client::{LoadedLibrary, ProcMacroClient, ProcMacroClientError};
pub use discovery::{
    DiscoveredCrate, DiscoveryError, DiscoveryReport, ProcMacroSource, Provenance,
};
pub use executor::{InProcessExecutor, InProcessProcMacro};
pub use expand::{core_to_wire, expand_proc_macro, wire_diagnostics_to_errors, wire_to_core};
pub use manifest::{
    DylibSection, HOST_TRIPLE, MANIFEST_EXTENSION, MANIFEST_FORMAT_VERSION, ManifestMacro,
    ProcMacroCrateManifest, fingerprint_dylib, sidecar_manifest_path,
};
pub use registry::{ProcMacroDef, ProcMacroId, ProcMacroRegistry};
pub use resolver::{ProcMacroResolver, ProcMacroRuntime, ResolvedProcMacro};
pub use yelang_proc_macro_bridge::protocol::ProcMacroKind;
