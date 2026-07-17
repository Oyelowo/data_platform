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
mod cfg;
mod eager;
mod error;
mod expander;
mod hygiene;
mod matcher;
mod parse_macro_output;
mod paste;
pub mod proc_macro;
mod quote;
mod resolver;
mod transcribe;

pub use builtin_decorators::{BuiltinDecorator, DecoratorArgs, DecoratorResult, ReprKind};
pub use builtin_macros::{
    BuiltinMacro, expand_assert, expand_panic, expand_todo, expand_unreachable,
};
pub use cfg::{CfgOptions, CfgPredicate};
pub use eager::{
    EagerBuiltin, EagerContext, EnvProvider, FileLoader, MemoryEnvProvider, MemoryFileLoader,
    StdEnvProvider, StdFileLoader, expand_eager_macros_in_stream,
};
pub use error::{DiagnosticLevel, ExpandError};
pub use expander::{
    ExpandResult, MacroExpander, expand_item, expand_program, expand_program_with_proc_macros,
};
pub use paste::{paste, paste_idents};
pub use proc_macro::{
    DiscoveredCrate, DiscoveryError, DiscoveryReport, DylibSection, HOST_TRIPLE, InProcessExecutor,
    InProcessProcMacro, LoadedLibrary, MANIFEST_EXTENSION, MANIFEST_FORMAT_VERSION, ManifestMacro,
    ProcMacroClient, ProcMacroClientError, ProcMacroCrateManifest, ProcMacroDef, ProcMacroId,
    ProcMacroKind, ProcMacroRegistry, ProcMacroResolver, ProcMacroRuntime, ProcMacroSource,
    Provenance, ResolvedProcMacro, core_to_wire, expand_proc_macro, fingerprint_dylib,
    sidecar_manifest_path, wire_diagnostics_to_errors, wire_to_core,
};
pub use quote::{
    binary, block, call, concat, ident, if_expr, int_lit, let_stmt, paren, path, punct,
    punct_joint, str_lit, unary,
};
pub use yelang_macro_core::{
    Delimiter, ExpnId, ExpnKind, Group, HygieneData, Ident, LitKind, Literal, MacroDefId, Punct,
    Spacing, Span, StrKind, SyntaxContextId, TokenId, TokenStream, TokenTree, Transparency,
};
