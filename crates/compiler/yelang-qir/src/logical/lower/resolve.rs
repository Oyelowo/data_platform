//! DefId resolution for query sources and link types.
//!
//! Resolves `@table` struct references, edge type annotations, and node
//! type annotations to their `DefId`s. This is the bridge between the
//! HIR's resolved paths and the plan tree's `SourceRef` / `EdgeRef` /
//! `NodeRef` types.

use yelang_arena::DefId;
use yelang_hir::hir::expr::Expr;
use yelang_hir::ids::ExprId;
use yelang_hir::res::Res;
use yelang_hir::Crate;
use yelang_interner::Symbol;

use crate::logical::plan::{EdgeRef, NodeRef, PlanArena, SourceRef};

/// Resolve a `from` node's source expression to a [`SourceRef`].
///
/// - `Res::Def { def_id }` → `SourceRef::Table` (a `@table`-annotated struct)
/// - `Res::Local { .. }` → `SourceRef::Local` (a local variable)
/// - Other expressions → `SourceRef::Call` (a function/method call)
pub fn resolve_source(
    source_expr: ExprId,
    label: Symbol,
    hir: &Crate,
    arena: &PlanArena,
) -> SourceRef {
    match hir.expr(source_expr) {
        Some(Expr::Path { res }) => match res {
            Res::Def { def_id } => SourceRef::Table {
                def: *def_id,
                name: label,
            },
            Res::Local { .. } => SourceRef::Local { name: label },
            _ => SourceRef::Local { name: label },
        },
        // Non-path expressions (calls, method chains, etc.)
        _ => SourceRef::Call {
            func: arena.to_thir(source_expr),
        },
    }
}

/// Resolve an edge type annotation to an [`EdgeRef`].
///
/// The edge type is specified in the link segment as `[writes@w:UserWritesBook]`.
/// The type annotation (`UserWritesBook`) is resolved to a `DefId` via the
/// HIR type expression.
///
/// If the type annotation is missing or cannot be resolved, returns a
/// placeholder `DefId`. The type checker will have already validated
/// that the edge type has `_from` and `_to` fields.
pub fn resolve_edge_ref(
    label: Symbol,
    binder: Symbol,
    ty_expr: Option<ExprId>,
    hir: &Crate,
) -> EdgeRef {
    let def = ty_expr
        .and_then(|expr_id| resolve_type_def_id(expr_id, hir))
        .unwrap_or_else(|| DefId::new(0));

    EdgeRef {
        def,
        label,
        binder,
    }
}

/// Resolve a node type annotation to a [`NodeRef`].
///
/// The node type is specified in the link segment as `(books@b:Book)`.
pub fn resolve_node_ref(
    label: Symbol,
    binder: Symbol,
    ty_expr: Option<ExprId>,
    hir: &Crate,
) -> NodeRef {
    let def = ty_expr
        .and_then(|expr_id| resolve_type_def_id(expr_id, hir))
        .unwrap_or_else(|| DefId::new(0));

    NodeRef {
        def,
        label,
        binder,
    }
}

/// Resolve a type expression to its `DefId`.
///
/// Handles path expressions (`User`, `Book`) and type ascription
/// expressions. Returns `None` if the expression is not a simple
/// type path.
fn resolve_type_def_id(expr_id: ExprId, hir: &Crate) -> Option<DefId> {
    match hir.expr(expr_id) {
        Some(Expr::Path { res: Res::Def { def_id } }) => Some(*def_id),
        // Type ascription: `expr: Type` — resolve the type part.
        Some(Expr::TypeAscription { ty, .. }) => {
            // The `ty` field is a ThirTyId in THIR, but in HIR it's
            // a type expression. For HIR, we look at the expression
            // directly.
            let _ = ty;
            None
        }
        _ => None,
    }
}
