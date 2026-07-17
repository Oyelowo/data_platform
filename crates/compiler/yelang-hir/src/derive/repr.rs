//! Built-in `#[repr(...)]` and `#[packed]` attributes.

use crate::derive::context::DeriveContext;
use crate::derive::error::DeriveError;

/// Process `#[repr(...)]` and `#[packed]` attributes on structs and enums.
///
/// For now this validates the attributes and emits diagnostics for unknown or
/// conflicting repr hints. The actual layout backend does not yet consume these
/// hints; this pass exists so that valid attributes are accepted and invalid
/// ones are rejected early.
pub fn expand_repr_attributes(ctx: &mut DeriveContext<'_, '_>) {
    let repr_sym = ctx.intern("repr");
    let packed_sym = ctx.intern("packed");

    let mut saw_c = false;
    let mut saw_packed = false;

    for attr in &ctx.ast_item.attributes {
        let Some(first) = attr.path.first() else {
            continue;
        };
        if first.symbol == repr_sym {
            match &attr.args {
                yelang_ast::AttributeArgs::Positional(args) if args.len() == 1 => {
                    if let yelang_ast::ExprKind::Literal(yelang_ast::Literal::Str(s)) =
                        &args[0].kind
                    {
                        let value = ctx.ctx.interner.resolve(&s.value);
                        match value {
                            "C" => saw_c = true,
                            other => {
                                ctx.error(DeriveError::BadAttributeArgs {
                                    attribute: repr_sym,
                                    reason: format!("unknown repr `{other}`"),
                                    span: attr.span,
                                });
                            }
                        }
                    } else {
                        ctx.error(DeriveError::BadAttributeArgs {
                            attribute: repr_sym,
                            reason: "`@repr` expects a string literal argument".to_string(),
                            span: attr.span,
                        });
                    }
                }
                _ => {
                    ctx.error(DeriveError::BadAttributeArgs {
                        attribute: repr_sym,
                        reason: "`@repr` expects a single string literal argument".to_string(),
                        span: attr.span,
                    });
                }
            }
        }
        if first.symbol == packed_sym {
            saw_packed = true;
        }
    }

    if saw_c && saw_packed {
        ctx.error(DeriveError::BadAttributeArgs {
            attribute: repr_sym,
            reason: "`@repr(C)` and `@packed` are conflicting layout hints".to_string(),
            span: ctx.derive_span,
        });
    }
}
