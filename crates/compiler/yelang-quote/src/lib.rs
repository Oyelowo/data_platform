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
/// * `#( #expr )*`, `#( #expr )+`, `#( #expr ),*`, `#( #expr ),+` repeat over an
///   iterable of `ToTokens` items, with an optional single-token separator.
/// * Multiple interpolations may appear inside one repetition, e.g.
///   `#(#name: #ty),*`.
/// * Repetitions may be nested.
///
/// Note: `#` is reserved for interpolation. To emit a literal `#` character,
/// construct a `Punct` and interpolate it, e.g. `quote!(#hash)` where
/// `hash` is `Punct::new('#', Spacing::Alone, span)`.
#[proc_macro]
pub fn quote(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    emit::expand(input)
}

/// Same as `quote!`, but applies the given span to every token that originates
/// within the macro invocation. Interpolated tokens keep their own spans.
///
/// # Syntax
///
/// ```ignore
/// use yelang_proc_macro::quote_spanned;
///
/// let tokens = quote_spanned!(span=> #name: Copy);
/// ```
#[proc_macro]
pub fn quote_spanned(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    emit::expand_spanned(input)
}
