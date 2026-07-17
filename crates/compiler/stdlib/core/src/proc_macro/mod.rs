//! The `proc_macro` module for Yelang.
//!
//! This is the standard-library API used by authors of Yelang procedural
//! macros. It is implemented by re-exporting the canonical token types from
//! `yelang_proc_macro` so that there is a single source of truth for the
//! token-tree representation.
//!
//! Macro authors write:
//!
//! ```ignore
//! use proc_macro::{TokenStream, quote};
//!
//! #[yelang_proc_macro::macro_export]
//! pub fn my_derive(item: TokenStream) -> TokenStream {
//!     quote! { /* ... */ }
//! }
//! ```
//!
//! The `quote!` and `quote_spanned!` macros are compiler built-ins; they are
//! not exported from this module (they are invoked by name).

pub use yelang_proc_macro::{
    Delimiter, Diagnostic, Group, Ident, Level, LineColumn, Literal, Punct, SourceFile, Spacing,
    Span, TokenStream, TokenTree,
};

pub use yelang_proc_macro::to_tokens::ToTokens;

#[cfg(test)]
mod tests;
