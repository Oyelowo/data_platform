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
 * - User-defined macros (future) will use the same infrastructure.
 * - Expansion is iterative: new macro invocations from expanded output are
 *   themselves expanded until a fixed point is reached.
 */

mod builtin_decorators;
mod builtin_macros;
mod expander;
mod hygiene;

pub use builtin_decorators::{
    BuiltinDecorator, DecoratorArgs, DecoratorResult, ReprKind,
};
pub use builtin_macros::{
    BuiltinMacro, expand_assert, expand_panic, expand_todo, expand_unreachable,
};
pub use expander::{
    ExpandError, MacroExpander, ExpandResult,
};
pub use hygiene::{
    ExpnId, HygieneData, SyntaxContext,
};

use yelang_ast::{Item, Program};
use yelang_interner::Interner;

/// Expand all macros and decorators in a program, returning the fully-expanded AST.
///
/// This is the primary entry point for the macro expansion phase.
/// It runs the expander iteratively until no more macro invocations remain.
pub fn expand_program(program: &Program, interner: &Interner) -> ExpandResult {
    let mut expander = MacroExpander::new(interner);
    expander.expand(program)
}

/// Expand macros and decorators on a single item.
///
/// Returns a vec because decorators such as `@derive` may generate
/// additional items (e.g. `impl` blocks) alongside the original item.
pub fn expand_item(item: &Item, interner: &Interner) -> Result<Vec<Item>, ExpandError> {
    let mut expander = MacroExpander::new(interner);
    expander.expand_item(item.clone())
}
