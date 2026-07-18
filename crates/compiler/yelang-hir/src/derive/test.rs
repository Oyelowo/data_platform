//! Built-in `#[test]` and `#[ignore]` attributes.

use crate::derive::context::DeriveContext;
use crate::derive::error::DeriveError;

/// Process `#[test]` and `#[ignore]` attributes on functions.
///
/// For now this validates the attributes and records them on the HIR item;
/// the actual test collection and runner integration lives outside HIR lowering.
pub fn expand_test_attributes(ctx: &mut DeriveContext<'_, '_>) {
    let test_sym = ctx.intern("test");
    let ignore_sym = ctx.intern("ignore");

    let mut saw_test = false;
    let mut saw_ignore = false;

    for attr in &ctx.ast_item.attributes {
        let Some(first) = attr.path.first() else {
            continue;
        };
        if first.symbol == test_sym {
            saw_test = true;
            if !matches!(&ctx.hir_item.kind, crate::hir::core::ItemKind::Fn { .. }) {
                ctx.error(DeriveError::UnsupportedItem {
                    derive: test_sym,
                    item_kind: crate::derive::context::item_kind_name(&ctx.hir_item.kind),
                    span: attr.span,
                });
            }
        }
        if first.symbol == ignore_sym {
            saw_ignore = true;
        }
    }

    if saw_ignore && !saw_test {
        ctx.error(DeriveError::InvalidShape {
            derive: ignore_sym,
            reason: "`@ignore` is only meaningful on `@test` functions".to_string(),
            span: ctx.derive_span,
        });
    }
}
