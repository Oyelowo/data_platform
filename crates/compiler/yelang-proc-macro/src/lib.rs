/*!
 * Yelang Proc-Macro
 *
 * Public API for writing procedural macros in Yelang. This crate is the only
 * dependency a macro author needs: it provides token types, spans, hygiene,
 * the `quote!` macro, lightweight parsing helpers, and diagnostic emission.
 *
 * Design principles:
 * - Token-based API for forward compatibility with language evolution.
 * - Hygiene is built in; identifiers cannot accidentally shadow user code.
 * - Diagnostics carry spans and are rendered by the compiler.
 * - `quote!` is a first-class built-in macro.
 */

pub mod api;
pub mod introspect;
pub mod parse;
pub mod quote;

pub use api::{
    Delimiter, Diagnostic, Group, Ident, Level, LineColumn, Literal, Punct, SourceFile, Spacing,
    Span, TokenStream, TokenTree,
};
// `quote!` is exported at crate root via `#[macro_export]` in src/quote/mod.rs.
