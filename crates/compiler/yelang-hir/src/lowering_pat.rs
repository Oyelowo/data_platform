//! Lowering of AST patterns to HIR patterns.

use yelang_ast::{Expr as AstExpr, ExprKind as AstExprKind, Pattern as AstPat};

use crate::hir_pat::{BindingMode, FieldPat, Pat};
use crate::ids::PatId;
use crate::lowering::LoweringContext;

/// Lower an AST pattern to a HIR pattern, allocate it in the crate arena, and
/// return its `PatId`. Any variable bindings introduced by the pattern are
/// registered in the lowering context's local map.
pub fn lower_pat(ctx: &mut LoweringContext, pat: &AstPat) -> PatId {
    let span = pat.span;
    let kind = match &pat.pattern {
        yelang_ast::PatternKind::Wildcard => Pat::Wild,
        yelang_ast::PatternKind::Binding {
            name,
            mutability,
            subpattern,
        } => Pat::Binding {
            mode: match mutability {
                yelang_ast::Mutability::Mutable => BindingMode::ByRef {
                    mutability: yelang_ast::Mutability::Mutable,
                },
                yelang_ast::Mutability::Immutable => BindingMode::ByValue,
            },
            name: name.symbol,
            subpat: subpattern.as_ref().map(|p| lower_pat(ctx, p)),
        },
        yelang_ast::PatternKind::Tuple { patterns } => Pat::Tuple {
            pats: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Struct { path, fields, rest } => Pat::Struct {
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
        yelang_ast::PatternKind::Record { fields, rest } => Pat::Struct {
            res: crate::res::Res::Err,
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
        yelang_ast::PatternKind::Path(path) => Pat::Path {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::PatternKind::Literal(lit) => Pat::Lit { lit: lit.clone() },
        yelang_ast::PatternKind::TupleStruct { path, patterns } => Pat::TupleStruct {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
            pats: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Slice { patterns } => {
            return lower_slice_pat(ctx, patterns, span)
        }
        yelang_ast::PatternKind::Ref { pattern, is_mut } => {
            let mutability = if *is_mut {
                yelang_ast::Mutability::Mutable
            } else {
                yelang_ast::Mutability::Immutable
            };
            if let yelang_ast::PatternKind::Binding {
                name,
                subpattern,
                ..
            } = &pattern.pattern
            {
                Pat::Binding {
                    mode: BindingMode::ByRef { mutability },
                    name: name.symbol,
                    subpat: subpattern.as_ref().map(|p| lower_pat(ctx, p)),
                }
            } else {
                let inner = lower_pat(ctx, pattern);
                Pat::Ref {
                    pat: inner,
                    mutability,
                }
            }
        }
        yelang_ast::PatternKind::Or(pats) => Pat::Or {
            pats: pats.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Range(range) => Pat::Range {
            start: range
                .start
                .as_ref()
                .map(|e| lower_range_bound_pat(ctx, e)),
            end: range
                .end
                .as_ref()
                .map(|e| lower_range_bound_pat(ctx, e)),
            end_inclusive: range.op.is_inclusive(),
        },
        yelang_ast::PatternKind::Grouped(inner) => return lower_pat(ctx, inner),
        yelang_ast::PatternKind::Rest { .. } => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: "rest pattern `..` outside of a slice pattern".to_string(),
                span,
            });
            Pat::Err
        }
        yelang_ast::PatternKind::Absent => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: "absent pattern".to_string(),
                span,
            });
            Pat::Err
        }
    };

    let pat_id = ctx.crate_hir.alloc_pat(kind, span);
    match ctx.crate_hir.pats.get(pat_id).expect("just allocated pattern") {
        Pat::Binding { name, .. } => {
            ctx.push_local(*name, pat_id);
        }
        Pat::Rest { name: Some(name) } => {
            ctx.push_local(*name, pat_id);
        }
        _ => {}
    }
    pat_id
}

/// Lower a slice pattern, splitting it into prefix, optional middle rest, and
/// suffix. Emits an error if there is more than one rest pattern.
fn lower_slice_pat(
    ctx: &mut LoweringContext,
    patterns: &[AstPat],
    span: yelang_lexer::Span,
) -> PatId {
    let mut rest_idx = None;
    for (i, p) in patterns.iter().enumerate() {
        if matches!(p.pattern, yelang_ast::PatternKind::Rest { .. }) {
            if rest_idx.is_some() {
                ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                    kind: "multiple rest patterns `..` in a single slice pattern".to_string(),
                    span,
                });
                return ctx.crate_hir.alloc_pat(Pat::Err, span);
            }
            rest_idx = Some(i);
        }
    }

    let kind = if let Some(rest_idx) = rest_idx {
        let prefix: Vec<PatId> = patterns[..rest_idx]
            .iter()
            .map(|p| lower_pat(ctx, p))
            .collect();
        let middle = Some(lower_rest_pat(ctx, &patterns[rest_idx]));
        let suffix: Vec<PatId> = patterns[rest_idx + 1..]
            .iter()
            .map(|p| lower_pat(ctx, p))
            .collect();
        Pat::Slice {
            prefix,
            middle,
            suffix,
        }
    } else {
        Pat::Slice {
            prefix: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
            middle: None,
            suffix: vec![],
        }
    };

    ctx.crate_hir.alloc_pat(kind, span)
}

/// Lower a rest pattern that occurs inside a slice pattern.
fn lower_rest_pat(ctx: &mut LoweringContext, pat: &AstPat) -> PatId {
    let span = pat.span;
    let name = match &pat.pattern {
        yelang_ast::PatternKind::Rest { name } => name.as_ref().map(|n| n.symbol),
        _ => unreachable!("lower_rest_pat called with non-rest pattern"),
    };
    let pat_id = ctx.crate_hir.alloc_pat(Pat::Rest { name }, span);
    if let Some(name) = name {
        ctx.push_local(name, pat_id);
    }
    pat_id
}

/// Lower the bound of a range pattern (a literal or path expression) into a
/// HIR pattern.
fn lower_range_bound_pat(ctx: &mut LoweringContext, expr: &AstExpr) -> PatId {
    let span = expr.span;
    let kind = match &expr.kind {
        AstExprKind::Literal(lit) => Pat::Lit { lit: lit.clone() },
        AstExprKind::Path(path) => Pat::Path {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        _ => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: "range pattern bound that is not a literal or path".to_string(),
                span,
            });
            return ctx.crate_hir.alloc_pat(Pat::Err, span);
        }
    };
    ctx.crate_hir.alloc_pat(kind, span)
}
