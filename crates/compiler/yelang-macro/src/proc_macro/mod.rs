//! Procedural macro integration.
//!
//! This module discovers procedural macros, communicates with the
//! yelang-proc-macro-server, and integrates expansion results back into the
//! declarative macro expander.

pub mod client;
pub mod discovery;
pub mod executor;
pub mod expand;
pub mod registry;
pub mod resolver;

pub use client::ProcMacroClient;
pub use discovery::ProcMacroDiscovery;
pub use executor::{InProcessExecutor, InProcessProcMacro};
pub use expand::expand_proc_macro;
pub use registry::{ProcMacroDef, ProcMacroId, ProcMacroRegistry};
pub use resolver::ProcMacroResolver;
