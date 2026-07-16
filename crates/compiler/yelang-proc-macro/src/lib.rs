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
pub mod bridge;
pub mod introspect;
pub mod parse;
pub mod to_tokens;

pub use api::{
    Delimiter, Diagnostic, Group, Ident, Level, LineColumn, Literal, Punct, SourceFile, Spacing,
    Span, TokenStream, TokenTree,
};
pub use bridge::{from_wire, into_wire};
pub use to_tokens::ToTokens;
pub use yelang_quote::{quote, quote_spanned};

// Re-export the C ABI types and symbols that `#[yelang_proc_macro::macro_export]`
// generated wrappers need, so macro authors only depend on this crate.
pub use bridge::{AttrMacroFn, DeriveMacroFn, FnLikeMacroFn};
pub use bridge::{run_attr_macro, run_derive_macro, run_fn_like_macro};
pub use yelang_proc_macro_bridge::abi::registrar::{
    CURRENT_ABI_VERSION, YelangAllocFn, YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro,
    YelangFreeFn, YelangMacroDescriptor, YelangMacroInvoke, YelangProcMacroEntry,
    YelangProcMacroExports, YelangProcMacroKind,
};
