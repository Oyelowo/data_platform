//! Lowering of AST patterns to HIR patterns.

use yelang_ast::Pattern as AstPat;

use crate::hir_pat::{BindingMode, FieldPat, Pat, PatKind};
use crate::lowering::LoweringContext;

/// Lower an AST pattern to a HIR pattern.
pub fn lower_pat(ctx: &mut LoweringContext, pat: &AstPat) -> Pat {
    let span = pat.span;
    let kind = match &pat.pattern {
        yelang_ast::PatternKind::Wildcard => PatKind::Wild,
        yelang_ast::PatternKind::Binding {
            name,
            mutability,
            subpattern,
        } => {
            let hir_id = ctx.next_hir_id();
            ctx.push_local(name.symbol, hir_id);
            PatKind::Binding {
                mode: match mutability {
                    yelang_ast::Mutability::Mutable => BindingMode::ByRef {
                        mutability: yelang_ast::Mutability::Mutable,
                    },
                    yelang_ast::Mutability::Immutable => BindingMode::ByValue,
                },
                name: name.symbol,
                subpat: subpattern.as_ref().map(|p| Box::new(lower_pat(ctx, p))),
            }
        }
        yelang_ast::PatternKind::Tuple { patterns } => PatKind::Tuple {
            pats: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Struct { path, fields, rest } => PatKind::Struct {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
            fields: fields
                .iter()
                .map(|f| FieldPat {
                    ident: f.name,
                    pat: lower_pat(ctx, &f.pattern),
                    is_shorthand: f.is_shorthand,
                    span: f.pattern.span,
                })
                .collect(),
            rest: *rest,
        },
        yelang_ast::PatternKind::Path(path) => PatKind::Path {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::PatternKind::Literal(lit) => PatKind::Lit { lit: lit.clone() },
        yelang_ast::PatternKind::TupleStruct { path, patterns } => PatKind::TupleStruct {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
            pats: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Slice { patterns } => PatKind::Slice {
            prefix: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
            middle: None,
            suffix: vec![],
        },
        yelang_ast::PatternKind::Ref { pattern, is_mut } => {
            let inner = lower_pat(ctx, pattern);
            PatKind::Binding {
                mode: if *is_mut {
                    BindingMode::ByRef {
                        mutability: yelang_ast::Mutability::Mutable,
                    }
                } else {
                    BindingMode::ByRef {
                        mutability: yelang_ast::Mutability::Immutable,
                    }
                },
                name: yelang_interner::Symbol::from(0u32),
                subpat: Some(Box::new(inner)),
            }
        }
        yelang_ast::PatternKind::Or(pats) => PatKind::Or {
            pats: pats.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Rest { .. } => PatKind::Wild,
        _ => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: format!("pattern kind {:?}", std::mem::discriminant(&pat.pattern)),
                span,
            });
            PatKind::Err
        }
    };

    Pat {
        hir_id: ctx.next_hir_id(),
        kind,
        span,
    }
}
