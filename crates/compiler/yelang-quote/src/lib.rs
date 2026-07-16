//! The `quote!` macro for Yelang procedural macros.
//!
//! This crate is a normal Rust proc-macro. It is re-exported by
//! `yelang-proc-macro` so that macro authors can write:
//!
//! ```ignore
//! use yelang_proc_macro::quote;
//!
//! let tokens = quote! { fn #name() {} };
//! ```
//!
//! The generated code builds `yelang_proc_macro::TokenStream` values, so the
//! macro-author crate only needs to depend on `yelang-proc-macro`.

mod emit;
mod parse;

/// Quasi-quotation for Yelang token streams.
///
/// Supported features:
/// * Literal tokens become the corresponding Yelang tokens.
/// * `#ident` or `#(expr)` interpolates any value implementing
///   `yelang_proc_macro::ToTokens`.
/// * `#( #expr )*` and `#( #expr ),*` repeat over an iterable of `ToTokens`
///   items, with an optional single-token separator.
#[proc_macro]
pub fn quote(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    emit::expand(input)
}
