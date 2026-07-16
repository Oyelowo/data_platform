//! Request/response messages.

use serde::{Deserialize, Serialize};

use super::token::{WireDiagnostic, WireHygienePayload, WireSpan, WireTokenStream};
use crate::sandbox::Limits;

slotmap::new_key_type! {
    /// Handle to a loaded library in the server.
    pub struct LibraryHandle;
}

/// A message from the compiler to the proc-macro server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// Start the conversation.
    Handshake { protocol_version: u32 },
    /// Load a proc-macro dynamic library.
    LoadLibrary { path: String },
    /// Invoke a function-like macro.
    ExpandFnLike {
        library: LibraryHandle,
        macro_index: u32,
        input: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    },
    /// Invoke an attribute macro.
    ExpandAttr {
        library: LibraryHandle,
        macro_index: u32,
        args: WireTokenStream,
        item: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    },
    /// Invoke a derive macro.
    ExpandDerive {
        library: LibraryHandle,
        macro_index: u32,
        item: WireTokenStream,
        call_site: WireSpan,
        def_site: WireSpan,
        hygiene: WireHygienePayload,
        limits: Limits,
    },
    /// Unload a previously loaded library.
    UnloadLibrary { library: LibraryHandle },
    /// Ask the server to shut down cleanly.
    Shutdown,
}

/// A message from the proc-macro server to the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    /// Handshake accepted.
    HandshakeAck { protocol_version: u32 },
    /// Library loaded; includes descriptors for each exported macro.
    LibraryLoaded {
        library: LibraryHandle,
        macros: Vec<MacroDescriptor>,
    },
    /// Expansion succeeded.
    Expanded {
        output: WireTokenStream,
        hygiene: WireHygienePayload,
    },
    /// Expansion produced a diagnostic.
    Diagnostic { diagnostic: WireDiagnostic },
    /// The macro panicked.
    Panic { message: String },
    /// A server-side error.
    Error { code: ErrorCode, message: String },
}

/// Description of an exported macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroDescriptor {
    pub name: String,
    pub kind: ProcMacroKind,
}

/// Kind of procedural macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcMacroKind {
    FunctionLike,
    Attribute,
    Derive,
}

/// Server-side error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    LibraryNotFound,
    LibraryLoadFailed,
    MacroNotFound,
    InvalidInput,
    ExpansionTimeout,
    ExpansionTooLarge,
    ExpansionMemoryLimit,
    ProtocolMismatch,
    Internal,
}
