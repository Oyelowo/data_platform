//! Wire representation of token streams.

use serde::{Deserialize, Serialize};

/// A token stream serialized for the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireTokenStream {
    pub trees: Vec<WireTokenTree>,
}

/// A single token tree on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireTokenTree {
    Group {
        delimiter: WireDelimiter,
        span: WireSpan,
        trees: Vec<WireTokenTree>,
    },
    Ident {
        text: String,
        span: WireSpan,
        is_raw: bool,
    },
    Punct {
        ch: char,
        spacing: WireSpacing,
        span: WireSpan,
    },
    Literal {
        text: String,
        kind: WireLitKind,
        span: WireSpan,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireDelimiter {
    Parenthesis,
    Brace,
    Bracket,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireSpacing {
    Alone,
    Joint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireLitKind {
    Int,
    Float,
    Str,
    Char,
    Bool,
    ByteStr,
    Byte,
}

/// A span on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WireSpan {
    pub lo: u32,
    pub hi: u32,
    pub file: u32,
    pub syntax_context: u32,
}

/// Hygiene data sent alongside a token stream so the server can reconstruct
/// spans with their full syntax-context chains, and so the compiler can
/// reconstruct contexts returned by the macro.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireHygienePayload {
    /// Syntax contexts referenced by tokens in the accompanying stream.
    pub contexts: Vec<WireSyntaxContext>,
    /// Expansion data for every `ExpnId` referenced by the contexts.
    pub expansions: Vec<WireExpnData>,
}

impl WireHygienePayload {
    /// An empty payload for the root context.
    pub fn empty() -> Self {
        Self {
            contexts: Vec::new(),
            expansions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSyntaxContext {
    pub id: u32,
    pub parent: Option<u32>,
    pub outer_expn: Option<u64>,
    pub transparency: WireTransparency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireExpnData {
    pub id: u64,
    pub parent: u64,
    pub call_site: WireSpan,
    pub def_site: WireSpan,
    pub kind: WireExpnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireTransparency {
    Opaque,
    Transparent,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireExpnKind {
    Root,
    MacroRules,
    Macro,
    ProcMacro,
    Comptime,
    AstPass,
}

/// A diagnostic on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireDiagnostic {
    pub level: WireLevel,
    pub message: String,
    pub spans: Vec<WireDiagnosticSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireDiagnosticSpan {
    pub span: WireSpan,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireLevel {
    Error,
    Warning,
    Note,
    Help,
}

/// Result of a single macro expansion returned across the dylib boundary.
///
/// The C ABI functions return a serialized `WireExpansionResult` rather than a
/// bare `WireTokenStream` so that procedural macros can emit structured
/// diagnostics alongside their output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireExpansionResult {
    pub output: WireTokenStream,
    pub diagnostics: Vec<WireDiagnostic>,
    pub hygiene: WireHygienePayload,
}

impl WireExpansionResult {
    /// Result with no output, diagnostics, or hygiene data.
    pub fn empty() -> Self {
        Self {
            output: WireTokenStream { trees: Vec::new() },
            diagnostics: Vec::new(),
            hygiene: WireHygienePayload::empty(),
        }
    }
}
