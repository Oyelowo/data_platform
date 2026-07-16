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
}

/// A span on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WireSpan {
    pub lo: u32,
    pub hi: u32,
    pub file: u32,
    pub syntax_context: u32,
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
}
