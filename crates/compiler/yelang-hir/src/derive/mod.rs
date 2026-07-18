//! Built-in derive and attribute expansion.
//!
//! This module implements compiler-native derives (e.g., `Copy`, `Clone`) and
//! built-in attributes (e.g., `#[test]`). It operates on the HIR produced by
//! AST lowering and synthesizes additional HIR items (impl blocks, test
//! metadata) directly, without any token-based macro expansion.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

use crate::hir::Item;
use crate::hir_item::{Item as HirItem, ItemKind};
use crate::lowering::LoweringContext;

mod clone;
mod copy;
mod debug;
mod eq;
pub mod error;
mod partial_eq;
mod repr;
mod test;

pub mod context;
pub mod helpers;

pub use context::DeriveContext;
pub use error::DeriveError;

/// A built-in derive expansion function.
pub type DeriveFn = fn(&mut DeriveContext<'_, '_>, &[Symbol]) -> Option<Item>;

/// Register all built-in derives in the given registry using the provided interner.
pub fn register_builtins(
    registry: &mut FxHashMap<Symbol, DeriveFn>,
    interner: &yelang_interner::Interner,
) {
    let names: &[(&str, DeriveFn)] = &[
        ("Copy", copy::derive_copy),
        ("Clone", clone::derive_clone),
        ("Debug", debug::derive_debug),
        ("PartialEq", partial_eq::derive_partial_eq),
        ("Eq", eq::derive_eq),
    ];
    for (name, func) in names {
        registry.insert(interner.get_or_intern(name), *func);
    }
}

/// Expand all built-in derives and attributes attached to an AST/HIR item pair.
///
/// Called from `lowering_item` after the item itself has been lowered.
/// Generated impl items are inserted into the crate's item map and impl list.
pub fn expand_item_derives(
    ctx: &mut LoweringContext<'_>,
    ast_item: &yelang_ast::Item,
    hir_item: &HirItem,
) {
    let derive_sym = ctx.interner.get_or_intern("derive");
    let mut registry = FxHashMap::default();
    register_builtins(&mut registry, ctx.interner);

    // First process non-derive attributes.
    let mut derive_ctx = DeriveContext {
        hir_item,
        ast_item,
        ctx,
        derive_span: hir_item.span,
        derive_name: derive_sym,
    };
    test::expand_test_attributes(&mut derive_ctx);
    repr::expand_repr_attributes(&mut derive_ctx);

    // Then process derive attributes.
    for attr in &ast_item.attributes {
        if attr.path.first().map(|i| i.symbol) != Some(derive_sym) {
            continue;
        }
        let derive_names = match &attr.args {
            yelang_ast::AttributeArgs::Positional(args) => args,
            _ => {
                ctx.error(
                    error::DeriveError::BadAttributeArgs {
                        attribute: derive_sym,
                        reason: "`@derive` expects a list of trait names".to_string(),
                        span: attr.span,
                    }
                    .into(),
                );
                continue;
            }
        };

        let names: Vec<Symbol> = derive_names
            .iter()
            .filter_map(|expr| derive_name_from_expr(ctx, expr))
            .collect();

        for name in &names {
            let derive_span = attr.span;
            let mut derive_ctx = DeriveContext {
                hir_item,
                ast_item,
                ctx,
                derive_span,
                derive_name: *name,
            };

            let func = match registry.get(name) {
                Some(f) => *f,
                None => {
                    derive_ctx.error(error::DeriveError::UnknownDerive {
                        name: *name,
                        span: derive_span,
                    });
                    continue;
                }
            };

            if let Some(generated_item) = func(&mut derive_ctx, &names) {
                let def_id = generated_item.def_id;
                if let ItemKind::Impl {
                    items,
                    generics,
                    self_ty,
                    of_trait,
                    polarity: _,
                } = &generated_item.kind
                {
                    ctx.crate_hir.impls.push(crate::hir::Impl {
                        generics: generics.clone(),
                        self_ty: self_ty.clone(),
                        of_trait: of_trait.clone(),
                        items: items.clone(),
                        polarity: crate::hir::ImplPolarity::Positive,
                        span: generated_item.span,
                    });
                }
                ctx.crate_hir.items.insert(def_id, Some(generated_item));
            }
        }
    }
}

/// Extract a derive name from an attribute argument expression.
fn derive_name_from_expr(_ctx: &LoweringContext<'_>, expr: &yelang_ast::Expr) -> Option<Symbol> {
    match &expr.kind {
        yelang_ast::ExprKind::Path(path) => {
            let segment = path.segments.last()?;
            Some(segment.ident.symbol)
        }
        yelang_ast::ExprKind::Literal(yelang_ast::Literal::Str(s)) => Some(s.value),
        _ => None,
    }
}
