/*
 * yelang-macro: Compile-time metaprogramming for Yelang.
 *
 * This crate implements the macro expansion phase of the compiler pipeline.
 * It operates on the AST after parsing but before name resolution, expanding
 * macro invocations and applying decorators to produce a fully-expanded AST.
 *
 * Design principles:
 * - All expansion is hygienic (names from macros cannot collide with user code).
 * - Built-in macros and decorators are first-class citizens of the expander.
 * - User-defined macros use the modern `macro name {}` declarative form.
 * - Expansion is iterative: new macro invocations from expanded output are
 *   themselves expanded until a fixed point is reached.
 * - Every token carries a unique `TokenId` for provenance.
 */

#![deny(unused_imports, unused_variables, dead_code, ambiguous_glob_imports)]

mod builtin_decorators;
mod builtin_macros;
mod error;
mod expander;
mod matcher;
mod paste;
mod quote;
mod resolver;
mod transcribe;

pub use builtin_decorators::{BuiltinDecorator, DecoratorArgs, DecoratorResult, ReprKind};
pub use builtin_macros::{
    BuiltinMacro, expand_assert, expand_panic, expand_todo, expand_unreachable,
};
pub use error::ExpandError;
pub use expander::{ExpandResult, MacroExpander, expand_item, expand_program};
pub use paste::{paste, paste_idents};
pub use quote::{
    binary, block, call, concat, ident, if_expr, int_lit, let_stmt, paren, path, punct,
    punct_joint, str_lit, unary,
};
pub use yelang_macro_core::{
    Delimiter, ExpnId, ExpnKind, Group, HygieneData, Ident, LitKind, Literal, MacroDefId, Punct,
    Spacing, Span, StrKind, SyntaxContextId, TokenId, TokenStream, TokenTree, Transparency,
};
