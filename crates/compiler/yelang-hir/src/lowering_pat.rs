//! Lowering of AST patterns to HIR patterns.

use yelang_ast::Pattern as AstPat;

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
        yelang_ast::PatternKind::Path(path) => Pat::Path {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
        },
        yelang_ast::PatternKind::Literal(lit) => Pat::Lit { lit: lit.clone() },
        yelang_ast::PatternKind::TupleStruct { path, patterns } => Pat::TupleStruct {
            res: crate::lowering_expr::resolve_ast_path(ctx, path),
            pats: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Slice { patterns } => Pat::Slice {
            prefix: patterns.iter().map(|p| lower_pat(ctx, p)).collect(),
            middle: None,
            suffix: vec![],
        },
        yelang_ast::PatternKind::Ref { pattern, is_mut } => {
            let inner = lower_pat(ctx, pattern);
            Pat::Binding {
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
                subpat: Some(inner),
            }
        }
        yelang_ast::PatternKind::Or(pats) => Pat::Or {
            pats: pats.iter().map(|p| lower_pat(ctx, p)).collect(),
        },
        yelang_ast::PatternKind::Rest { .. } => Pat::Wild,
        _ => {
            ctx.error(crate::lowering_err::LoweringError::UnsupportedAst {
                kind: format!("pattern kind {:?}", std::mem::discriminant(&pat.pattern)),
                span,
            });
            Pat::Err
        }
    };

    let pat_id = ctx.crate_hir.alloc_pat(kind, span);
    if let Pat::Binding { name, .. } = ctx
        .crate_hir
        .pats
        .get(pat_id)
        .expect("just allocated pattern")
    {
        ctx.push_local(*name, pat_id);
    }
    pat_id
}
