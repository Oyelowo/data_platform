/*! Expression and statement type checking.
 *
 * The main type checker that infers types for all expressions and statements
 * within a function body.
 */

use yelang_ast::{AssignOpKind, BinaryOp};
use yelang_hir::hir::core::{Arm, Block, Expr, FieldExpr, Stmt};
use yelang_hir::hir::ty::Ty as HirTy;
use yelang_hir::hir::expr::ComprehensionKind;
use yelang_hir::hir::query::{
    ConflictAction, CreateData, CreateLinkPath, CreateNode, CreateQuery, CreateEdge, DeleteQuery,
    LinkQuery, Query, QueryKind, SelectQuery, Setter, SetterOp, UnlinkPath, UnlinkQuery,
    UpdateMutation, UpdateQuery, UpsertQuery,
};
use yelang_hir::ids::{BodyId, ExprId, HirTyId, PatId, StmtId};
use yelang_hir::res::Res;
use yelang_infer::error::TypeError;
use yelang_ty::generic::GenericArg;
use yelang_ty::generic::Substitution;
use yelang_ty::primitive::IntTy;
use yelang_ty::subst::substitute;
use yelang_ty::ty::{AdtDef, AnonStructDef, InferTy, Mutability, Ty, TyId, TypeAndMut};

use crate::coerce::Coerce;
use crate::fn_ctxt::{BreakableKind, BreakableScope, FnCtxt};
use crate::hir_ty_lower::lower_hir_ty;
use crate::pat::check_pat;

/// Return the source span of a HIR expression.
pub(crate) fn expr_span(fcx: &FnCtxt<'_>, expr_id: ExprId) -> yelang_lexer::Span {
    fcx.tcx.crate_hir().expr_span(expr_id)
}

/// Type-check a function body.
pub fn check_body(fcx: &mut FnCtxt<'_>, body_id: BodyId) {
    fcx.push_scope();

    let body = fcx
        .tcx
        .crate_hir()
        .body(body_id)
        .expect("BodyId should be valid")
        .clone();

    // If the signature declared the return type as `_`, infer it from the body.
    if let Some(poly_sig) = fcx.tcx.fn_sig(fcx.results.def_id) {
        if poly_sig.sig.return_ty_infer {
            fcx.return_ty = fcx.new_ty_var();
        }
    }

    // Check parameters: introduce local variables for each param
    for param in &body.params {
        let param_ty = lower_hir_ty_id(fcx, param.ty);
        check_pat(fcx, param.pat, param_ty);
    }

    // Check the body expression
    let body_ty = check_expr(fcx, body.value);

    // Coerce body type to return type
    if let Err(()) = fcx.coerce(body_ty, fcx.return_ty) {
        fcx.report_mismatch(body.span, fcx.return_ty, body_ty);
    }

    // Prove trait/well-formedness obligations accumulated during checking.
    let unproven = fcx.prove_obligations();
    for obligation in unproven {
        fcx.report_obligation_error(body.span, obligation);
    }

    // Write final inferred types back, resolving remaining variables.
    crate::writeback::writeback_types(fcx);

    fcx.pop_scope();
}

/// Type-check an expression and return its inferred type.
pub fn check_expr(fcx: &mut FnCtxt<'_>, expr_id: ExprId) -> TyId {
    let expr = fcx
        .tcx
        .crate_hir()
        .expr(expr_id)
        .expect("ExprId should be valid")
        .clone();
    let ty = check_expr_value(fcx, &expr, expr_id);
    fcx.record_expr_ty(expr_id, ty);
    ty
}

fn check_expr_value(fcx: &mut FnCtxt<'_>, expr: &Expr, _expr_id: ExprId) -> TyId {
    match expr {
        Expr::Lit { lit } => check_literal(fcx, lit),
        Expr::Path { res } => check_path(fcx, res),
        Expr::Binary { op, left, right } => check_binary(fcx, *op, *left, *right),
        Expr::Unary { op, expr } => check_unary(fcx, *op, *expr),
        Expr::Call { func, args } => check_call(fcx, *func, args),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => check_method_call(fcx, *receiver, method, args),
        Expr::Field { expr, field } => check_field(fcx, *expr, field),
        Expr::Index { expr, index } => check_index(fcx, *expr, *index),
        Expr::Assign { left, right } => check_assign(fcx, *left, *right),
        Expr::Block { block } => check_block(fcx, block),
        Expr::Loop { block, label } => check_loop(fcx, block, label.as_ref()),
        Expr::Break { label, expr } => check_break(fcx, label.as_ref(), *expr),
        Expr::Continue { label } => check_continue(fcx, label.as_ref()),
        Expr::Return { expr } => check_return(fcx, *expr),
        Expr::Match { expr, arms } => check_match(fcx, *expr, arms),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => check_if(fcx, *cond, *then_branch, *else_branch),
        Expr::Let { pat, expr } => check_let_expr(fcx, *pat, *expr),
        Expr::Closure { params, body, .. } => check_closure(fcx, params, *body),
        Expr::Struct { path, fields, rest } => check_struct_literal(fcx, path, fields, *rest),
        Expr::Tuple { exprs } => check_tuple(fcx, exprs),
        Expr::Array { exprs } => check_array(fcx, exprs),
        Expr::Cast { expr, ty } => check_cast(fcx, *expr, *ty),
        Expr::AssignOp { left, right, op } => check_assign_op(fcx, op.clone(), *left, *right),
        Expr::DestructureAssign { pat, value } => {
            let value_ty = check_expr(fcx, *value);
            crate::pat::check_pat(fcx, *pat, value_ty);
            fcx.mk_unit()
        }
        Expr::Range { start, end, .. } => check_range(fcx, *start, *end),
        Expr::Object { fields } => check_object_literal(fcx, fields),
        Expr::IsType { expr: inner, ty } => {
            check_expr(fcx, *inner);
            lower_hir_ty_id(fcx, *ty)
        }
        Expr::TypeAscription { expr: inner, ty } => {
            let ascribed = lower_hir_ty_id(fcx, *ty);
            let inner_span = expr_span(fcx, *inner);
            let expr_ty = check_expr(fcx, *inner);
            if let Err(()) = fcx.coerce(expr_ty, ascribed) {
                fcx.report_type_error(
                    inner_span,
                    TypeError::Mismatch {
                        expected: ascribed,
                        found: expr_ty,
                    },
                );
            }
            ascribed
        }
        Expr::Try { expr: inner } => check_try(fcx, *inner),
        Expr::Await { expr: inner } => {
            check_expr(fcx, *inner);
            fcx.new_ty_var()
        }
        Expr::Async { body } => {
            check_body(fcx, *body);
            fcx.new_ty_var()
        }
        Expr::Gen { body, .. } => {
            check_body(fcx, *body);
            fcx.new_ty_var()
        }
        Expr::DocumentAccess { base, projection } => {
            check_expr(fcx, *base);
            for proj in projection {
                match proj {
                    yelang_hir::hir::expr::DocumentProjection::Field { value, .. } => {
                        if let Some(e) = value {
                            check_expr(fcx, *e);
                        }
                    }
                    yelang_hir::hir::expr::DocumentProjection::Spread(e) => {
                        check_expr(fcx, *e);
                    }
                }
            }
            fcx.new_ty_var()
        }
        Expr::Comprehension {
            kind,
            element,
            variables,
            condition,
        } => check_comprehension(fcx, expr_span(fcx, _expr_id), *kind, *element, variables, condition.as_ref()),
        Expr::Query(query_id) => check_query(fcx, expr_span(fcx, _expr_id), *query_id),
        Expr::ArrayRepeat { value, count } => {
            check_array_repeat(fcx, expr_span(fcx, _expr_id), *value, *count)
        }
        Expr::Err => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Comprehension and query checking
// ---------------------------------------------------------------------------

fn check_comprehension(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    kind: ComprehensionKind,
    element: ExprId,
    variables: &[yelang_hir::hir::expr::ComprehensionVar],
    condition: Option<&ExprId>,
) -> TyId {
    for var in variables {
        let source_ty = check_expr(fcx, var.source);
        let mut elem_ty = fcx.expect_array(span, source_ty);
        for _ in 0..var.flatten {
            elem_ty = fcx.expect_array(span, elem_ty);
        }
        check_pat(fcx, var.pat, elem_ty);
    }

    let element_ty = check_expr(fcx, element);

    if let Some(cond) = condition {
        let cond_ty = check_expr(fcx, *cond);
        fcx.demand_eq(span, fcx.mk_bool(), cond_ty);
    }

    match kind {
        ComprehensionKind::List => fcx.mk_array_ty(element_ty),
        ComprehensionKind::Set | ComprehensionKind::Dict => {
            // TODO: distinct set/dict result types once they exist.
            let _ = kind;
            fcx.new_ty_var()
        }
    }
}

fn check_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    query_id: yelang_hir::ids::QueryId,
) -> TyId {
    let query = fcx
        .tcx
        .crate_hir()
        .query(query_id)
        .expect("QueryId should be valid")
        .clone();
    match &query.kind {
        QueryKind::Select(select) => {
            if select.from.len() != 1 {
                fcx.report_type_error(
                    span,
                    TypeError::Custom(
                        "multi-root `from` is not yet supported; use a single root".to_string(),
                    ),
                );
                return fcx.mk_error();
            }

            fcx.push_scope();
            let result = check_select_query(fcx, span, select);
            fcx.pop_scope();
            result
        }
        QueryKind::Create(create) => {
            fcx.push_scope();
            let result = check_create_query(fcx, span, create);
            fcx.pop_scope();
            result
        }
        QueryKind::Update(update) => {
            fcx.push_scope();
            let result = check_update_query(fcx, span, update);
            fcx.pop_scope();
            result
        }
        QueryKind::Upsert(upsert) => {
            fcx.push_scope();
            let result = check_upsert_query(fcx, span, upsert);
            fcx.pop_scope();
            result
        }
        QueryKind::Delete(delete) => {
            fcx.push_scope();
            let result = check_delete_query(fcx, span, delete);
            fcx.pop_scope();
            result
        }
        QueryKind::Link(link) => {
            fcx.push_scope();
            let result = check_link_query(fcx, span, link);
            fcx.pop_scope();
            result
        }
        QueryKind::Unlink(unlink) => {
            fcx.push_scope();
            let result = check_unlink_query(fcx, span, unlink);
            fcx.pop_scope();
            result
        }
    }
}

fn check_select_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    select: &SelectQuery,
) -> TyId {
    for from in &select.from {
        let source_ty = check_expr(fcx, from.source);

        let elem_ty = if let Some(elem_ty_id) = from.elem_ty {
            let annotated = lower_hir_ty_id(fcx, elem_ty_id);
            let expected_source = fcx.mk_array_ty(annotated);
            fcx.demand_eq(span, expected_source, source_ty);
            annotated
        } else {
            fcx.expect_array(span, source_ty)
        };

        check_pat(fcx, from.binder, elem_ty);

        if let Some(filter) = from.filter {
            let filter_ty = check_expr(fcx, filter);
            fcx.demand_eq(span, fcx.mk_bool(), filter_ty);
        }

        for part in &from.order_by {
            check_expr(fcx, part.expr);
        }

        if let Some(range) = &from.range {
            if let Some(start) = range.start {
                check_expr(fcx, start);
            }
            if let Some(end) = range.end {
                check_expr(fcx, end);
            }
        }
    }

    if let Some(where_clause) = select.where_clause {
        let where_ty = check_expr(fcx, where_clause);
        fcx.demand_eq(span, fcx.mk_bool(), where_ty);
    }

    for part in &select.order_by {
        check_expr(fcx, part.expr);
    }

    if let Some(range) = &select.range {
        if let Some(start) = range.start {
            check_expr(fcx, start);
        }
        if let Some(end) = range.end {
            check_expr(fcx, end);
        }
    }

    check_expr(fcx, select.projection)
}

fn check_create_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    create: &CreateQuery,
) -> TyId {
    let table_ty = lower_hir_ty_id(fcx, create.table);
    check_pat(fcx, create.binder, table_ty);

    match &create.data {
        CreateData::Object(fields) => {
            check_create_object_fields(fcx, table_ty, fields);
        }
        CreateData::Array(exprs) => {
            for expr in exprs {
                check_expr(fcx, *expr);
            }
        }
    }

    for path in &create.links {
        check_create_link_path(fcx, span, path);
    }

    if let Some(ret) = create.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

/// Check the fields of a `create`/`upsert` object payload against the declared
/// table type and report mismatches.
fn check_create_object_fields(
    fcx: &mut FnCtxt<'_>,
    table_ty: TyId,
    fields: &[(yelang_ast::Ident, ExprId)],
) {
    let interner = fcx.tcx.interner();
    let (def_id, args) = match interner.ty(table_ty) {
        Ty::Adt(AdtDef { def_id }, args) => (def_id, args),
        _ => {
            // Non-ADT table types can't be checked structurally here.
            for (_, expr) in fields {
                check_expr(fcx, *expr);
            }
            return;
        }
    };

    let Some(adt) = fcx.tcx.adt_def(def_id) else {
        for (_, expr) in fields {
            check_expr(fcx, *expr);
        }
        return;
    };

    let variant = match adt.variants.first() {
        Some(v) => v,
        None => {
            for (_, expr) in fields {
                check_expr(fcx, *expr);
            }
            return;
        }
    };

    let subst = Substitution::from_args(args.iter().copied().collect());

    for (field, expr) in fields {
        let field_ty = variant
            .fields
            .iter()
            .find(|f| f.ident.symbol == field.symbol)
            .map(|f| substitute(interner, f.ty, &subst));

        let expr_ty = check_expr(fcx, *expr);
        if let Some(expected) = field_ty {
            if let Err(()) = fcx.coerce(expr_ty, expected) {
                fcx.report_mismatch(expr_span(fcx, *expr), expected, expr_ty);
            }
        }
    }
}

fn check_update_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    update: &UpdateQuery,
) -> TyId {
    let table_ty = lower_hir_ty_id(fcx, update.table);
    check_pat(fcx, update.binder, table_ty);

    match &update.mutation {
        UpdateMutation::Merge(fields) => {
            check_create_object_fields(fcx, table_ty, fields);
        }
        UpdateMutation::Set(setters) => {
            for setter in setters {
                check_setter(fcx, span, setter);
            }
        }
    }

    for path in &update.links {
        check_create_link_path(fcx, span, path);
    }

    if let Some(cond) = update.condition {
        let cond_ty = check_expr(fcx, cond);
        fcx.demand_eq(span, fcx.mk_bool(), cond_ty);
    }

    if let Some(ret) = update.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

fn check_upsert_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    upsert: &UpsertQuery,
) -> TyId {
    let table_ty = lower_hir_ty_id(fcx, upsert.table);
    check_pat(fcx, upsert.binder, table_ty);

    match &upsert.data {
        CreateData::Object(fields) => {
            check_create_object_fields(fcx, table_ty, fields);
        }
        CreateData::Array(exprs) => {
            for expr in exprs {
                check_expr(fcx, *expr);
            }
        }
    }

    for path in &upsert.links {
        check_create_link_path(fcx, span, path);
    }

    if let Some(ret) = upsert.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

fn check_delete_query(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    delete: &DeleteQuery,
) -> TyId {
    let table_ty = lower_hir_ty_id(fcx, delete.table);
    check_pat(fcx, delete.binder, table_ty);

    if let Some(cond) = delete.condition {
        let cond_ty = check_expr(fcx, cond);
        fcx.demand_eq(span, fcx.mk_bool(), cond_ty);
    }

    if let Some(ret) = delete.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

fn check_link_query(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, link: &LinkQuery) -> TyId {
    for path in &link.paths {
        check_create_link_path(fcx, span, path);
    }

    if let Some(ret) = link.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

fn check_unlink_query(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, unlink: &UnlinkQuery) -> TyId {
    for path in &unlink.paths {
        check_unlink_path(fcx, span, path);
    }

    if let Some(ret) = unlink.return_ {
        check_expr(fcx, ret)
    } else {
        fcx.mk_unit()
    }
}

fn check_create_link_path(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    path: &CreateLinkPath,
) {
    for segment in &path.segments {
        match segment {
            yelang_hir::hir::query::CreatePathSegment::Node(node) => {
                check_create_node(fcx, span, node)
            }
            yelang_hir::hir::query::CreatePathSegment::Edge(edge) => {
                check_create_edge(fcx, span, edge)
            }
        }
    }
}

fn check_create_node(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, node: &CreateNode) {
    let table_ty = lower_hir_ty_id(fcx, node.table);
    check_pat(fcx, node.binder, table_ty);
    check_node_modifiers(fcx, span, &node.modifiers);
}

fn check_create_edge(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, edge: &CreateEdge) {
    let table_ty = lower_hir_ty_id(fcx, edge.table);
    check_pat(fcx, edge.binder, table_ty);
    for (_, expr) in &edge.data {
        check_expr(fcx, *expr);
    }
}

fn check_unlink_path(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, path: &UnlinkPath) {
    for segment in &path.segments {
        match segment {
            yelang_hir::hir::query::UnlinkPathSegment::Node(node) => {
                check_link_node(fcx, span, node)
            }
            yelang_hir::hir::query::UnlinkPathSegment::Edge(edge) => {
                check_link_edge(fcx, span, edge)
            }
        }
    }
}

fn check_link_node(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, node: &yelang_hir::hir::query::LinkNode) {
    if let Some(binder) = node.binder {
        if let Some(table) = node.table {
            let table_ty = lower_hir_ty_id(fcx, table);
            check_pat(fcx, binder, table_ty);
        }
    }
    check_node_modifiers(fcx, span, &node.modifiers);
}

fn check_link_edge(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, edge: &yelang_hir::hir::query::LinkEdge) {
    if let Some(binder) = edge.binder {
        if let Some(table) = edge.table {
            let table_ty = lower_hir_ty_id(fcx, table);
            check_pat(fcx, binder, table_ty);
        }
    }
    check_node_modifiers(fcx, span, &edge.modifiers);
}

fn check_node_modifiers(
    fcx: &mut FnCtxt<'_>,
    span: yelang_lexer::Span,
    modifiers: &yelang_hir::hir::query::NodeModifiers,
) {
    if let Some(filter) = modifiers.filter {
        let filter_ty = check_expr(fcx, filter);
        fcx.demand_eq(span, fcx.mk_bool(), filter_ty);
    }
    for part in &modifiers.order_by {
        check_expr(fcx, part.expr);
    }
    if let Some(range) = &modifiers.range {
        if let Some(start) = range.start {
            check_expr(fcx, start);
        }
        if let Some(end) = range.end {
            check_expr(fcx, end);
        }
    }
}

fn check_setter(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, setter: &Setter) {
    let path_ty = check_expr(fcx, setter.path);
    let value_ty = check_expr(fcx, setter.value);

    match setter.op {
        SetterOp::Assign => {
            if let Err(()) = fcx.coerce(value_ty, path_ty) {
                fcx.report_mismatch(span, path_ty, value_ty);
            }
        }
        SetterOp::Increment | SetterOp::Decrement => {
            // Path and value must be numeric. Demand equality as a simple check.
            fcx.demand_eq(span, path_ty, value_ty);
        }
    }
}

fn check_array_repeat(fcx: &mut FnCtxt<'_>, span: yelang_lexer::Span, value: ExprId, count: ExprId) -> TyId {
    let value_ty = check_expr(fcx, value);
    let count_ty = check_expr(fcx, count);
    // The repeat count must be an integer.  Use `i32` as the default fallback
    // for unresolved integer literals, mirroring the general integer fallback.
    fcx.demand_eq(span, fcx.mk_int(IntTy::I32), count_ty);

    let len_const = if let Some(lit) = fcx.tcx.crate_hir().expr(count) {
        match lit {
            Expr::Lit {
                lit: yelang_hir::hir::core::Lit::Int(il),
            } => {
                let s = il.value.to_string();
                if let Ok(v) = s.parse::<i128>() {
                    fcx.tcx.interner().mk_const_from_parts(
                        yelang_ty::ty::Const::Value(yelang_ty::ty::ConstValue::Int(v)),
                        fcx.mk_uint(yelang_ty::primitive::UintTy::Usize),
                    )
                } else {
                    fcx.tcx.interner().mk_const_from_parts(
                        yelang_ty::ty::Const::Error,
                        fcx.mk_uint(yelang_ty::primitive::UintTy::Usize),
                    )
                }
            }
            _ => fcx.tcx.interner().mk_const_from_parts(
                yelang_ty::ty::Const::Error,
                fcx.mk_uint(yelang_ty::primitive::UintTy::Usize),
            ),
        }
    } else {
        fcx.tcx.interner().mk_const_from_parts(
            yelang_ty::ty::Const::Error,
            fcx.mk_uint(yelang_ty::primitive::UintTy::Usize),
        )
    };

    fcx.mk_array(value_ty, len_const)
}

// ---------------------------------------------------------------------------
// Literal checking
// ---------------------------------------------------------------------------

fn check_literal(fcx: &mut FnCtxt<'_>, lit: &yelang_hir::hir::core::Lit) -> TyId {
    match lit {
        yelang_hir::hir::core::Lit::Int(_) => fcx.new_int_var(),
        yelang_hir::hir::core::Lit::Float(_) => fcx.new_float_var(),
        yelang_hir::hir::core::Lit::Bool(_) => fcx.mk_bool(),
        yelang_hir::hir::core::Lit::Char(_) => fcx.mk_char(),
        yelang_hir::hir::core::Lit::Str(_) => fcx.mk_str(),
        _ => {
            // TODO: define types for these literals
            fcx.new_ty_var()
        }
    }
}

// ---------------------------------------------------------------------------
// Path checking
// ---------------------------------------------------------------------------

fn check_path(fcx: &mut FnCtxt<'_>, res: &Res) -> TyId {
    match res {
        Res::Local { pat_id } => {
            if let Some(ty) = fcx.lookup_local(*pat_id) {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::Def { def_id } => {
            if let Some(ty) = fcx.item_ty(*def_id) {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::PrimTy { .. } => {
            // PrimTy shouldn't appear in expression position
            fcx.mk_error()
        }
        Res::SelfTy { .. } => {
            if let Some(ty) = fcx.self_ty {
                ty
            } else {
                fcx.mk_error()
            }
        }
        Res::SelfVal { .. } => {
            // self parameter type
            fcx.self_ty.unwrap_or_else(|| fcx.mk_error())
        }
        Res::Err => fcx.mk_error(),
    }
}

// ---------------------------------------------------------------------------
// Binary operator checking
// ---------------------------------------------------------------------------

fn check_binary(fcx: &mut FnCtxt<'_>, op: BinaryOp, left: ExprId, right: ExprId) -> TyId {
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);
    let right_span = expr_span(fcx, right);

    match op {
        // Arithmetic: both operands must be numeric, result is same type
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Modulo
        | BinaryOp::Power => {
            fcx.demand_eq(right_span, left_ty, right_ty);
            left_ty
        }
        // Bitwise: both operands must be integer, result is same type
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr => {
            fcx.demand_eq(right_span, left_ty, right_ty);
            left_ty
        }
        // Comparison: both operands must be comparable, result is bool
        BinaryOp::Eq
        | BinaryOp::Ne
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte
        | BinaryOp::Like
        | BinaryOp::ILike
        | BinaryOp::Regex
        | BinaryOp::In
        | BinaryOp::NotIn => {
            fcx.demand_eq(right_span, left_ty, right_ty);
            fcx.mk_bool()
        }
        // Logical: both operands must be bool, result is bool
        BinaryOp::And | BinaryOp::Or => {
            fcx.demand_eq(expr_span(fcx, left), left_ty, fcx.mk_bool());
            fcx.demand_eq(right_span, right_ty, fcx.mk_bool());
            fcx.mk_bool()
        }
    }
}

// ---------------------------------------------------------------------------
// Unary operator checking
// ---------------------------------------------------------------------------

fn check_unary(fcx: &mut FnCtxt<'_>, op: yelang_ast::UnaryOp, expr: ExprId) -> TyId {
    let expr_ty = check_expr(fcx, expr);
    let interner = fcx.tcx.interner();

    match op {
        yelang_ast::UnaryOp::Neg => {
            // Operand must be numeric; result is same type
            expr_ty
        }
        yelang_ast::UnaryOp::Not => {
            // Operand must be bool or integer; result is same type
            expr_ty
        }
        yelang_ast::UnaryOp::Deref => {
            // Operand must be pointer or reference; result is pointee
            let span = expr_span(fcx, expr);
            match interner.ty(expr_ty) {
                Ty::Ref(ty, _) | Ty::RawPtr(TypeAndMut { ty, .. }) => ty,
                Ty::Infer(InferTy::TyVar(_)) => {
                    let inner = fcx.new_ty_var();
                    let ptr = fcx.mk_ref(inner, Mutability::Not);
                    fcx.demand_eq(span, expr_ty, ptr);
                    inner
                }
                _ => {
                    fcx.report_type_error(
                        span,
                        TypeError::Custom(format!(
                            "cannot dereference a value of type `{:?}`",
                            expr_ty
                        )),
                    );
                    fcx.mk_error()
                }
            }
        }
        yelang_ast::UnaryOp::Ref => {
            // Result is &T (immutable reference)
            fcx.mk_ref(expr_ty, Mutability::Not)
        }
        yelang_ast::UnaryOp::RefMut => {
            // Result is &mut T
            fcx.mk_ref(expr_ty, Mutability::Mut)
        }
    }
}

// ---------------------------------------------------------------------------
// Call checking
// ---------------------------------------------------------------------------

fn check_call(fcx: &mut FnCtxt<'_>, func: ExprId, args: &[ExprId]) -> TyId {
    if let Some(ty) = crate::array_builtins::try_check_len_or_count_call(fcx, func, args) {
        return ty;
    }

    let func_span = expr_span(fcx, func);
    let func_ty = check_expr(fcx, func);
    let interner = fcx.tcx.interner();

    match interner.ty(func_ty) {
        Ty::FnPtr(sig) => {
            let inputs = &sig.sig.inputs;
            let output = sig.sig.output;

            if inputs.len() != args.len() {
                fcx.report_type_error(
                    func_span,
                    TypeError::ArgCount {
                        expected: inputs.len(),
                        found: args.len(),
                    },
                );
                return fcx.mk_error();
            }

            for (input, arg) in inputs.iter().zip(args.iter()) {
                let arg_span = expr_span(fcx, *arg);
                let arg_ty = check_expr(fcx, *arg);
                let expected = match input {
                    GenericArg::Type(t) => *t,
                    _ => {
                        fcx.report_type_error(
                            arg_span,
                            TypeError::GenericArgKindMismatch { index: 0 },
                        );
                        continue;
                    }
                };
                if fcx.coerce(arg_ty, expected).is_err() {
                    fcx.report_mismatch(arg_span, expected, arg_ty);
                }
            }

            output
        }
        Ty::FnDef(fd) => {
            // Function item: instantiate generic parameters with fresh inference
            // variables, check arguments against the substituted signature, and
            // record the callee's where-clause obligations.
            let poly_sig = match fcx.tcx.fn_sig(fd.def_id) {
                Some(sig) => sig,
                None => {
                    fcx.report_type_error(
                        func_span,
                        TypeError::Custom("missing signature for function item".into()),
                    );
                    return fcx.mk_error();
                }
            };

            let subst = fcx.fresh_substitution_for_generics(fd.def_id);
            let inputs = substitute(interner, poly_sig.sig.inputs, &subst);
            let output = substitute(interner, poly_sig.sig.output, &subst);
            let sig = yelang_ty::ty::FnSig {
                inputs,
                output,
                return_ty_infer: poly_sig.sig.return_ty_infer,
            };

            if sig.inputs.len() != args.len() {
                fcx.report_type_error(
                    func_span,
                    TypeError::ArgCount {
                        expected: sig.inputs.len(),
                        found: args.len(),
                    },
                );
                return fcx.mk_error();
            }

            for (input, arg) in sig.inputs.iter().zip(args.iter()) {
                let arg_span = expr_span(fcx, *arg);
                let arg_ty = check_expr(fcx, *arg);
                let expected = match input {
                    GenericArg::Type(t) => *t,
                    _ => {
                        fcx.report_type_error(
                            arg_span,
                            TypeError::GenericArgKindMismatch { index: 0 },
                        );
                        continue;
                    }
                };
                if fcx.coerce(arg_ty, expected).is_err() {
                    fcx.report_mismatch(arg_span, expected, arg_ty);
                }
            }

            // Emit substituted where-clause obligations.
            if let Some(generics) = fcx.tcx.generics_of(fd.def_id) {
                for &pred in &generics.predicates {
                    let pred = substitute(interner, pred, &subst);
                    fcx.emit_obligation(pred);
                }
            }

            sig.output
        }
        Ty::Infer(InferTy::TyVar(_)) => {
            // Function type not yet known: create expected arg types and return type
            let arg_tys: Vec<_> = args.iter().map(|arg| check_expr(fcx, *arg)).collect();
            let arg_args = fcx.tcx.interner().mk_generic_args(
                &arg_tys
                    .iter()
                    .map(|&t| GenericArg::Type(t))
                    .collect::<Vec<_>>(),
            );
            let ret_ty = fcx.new_ty_var();
            let expected = fcx.mk_fn_ptr(arg_args, ret_ty);
            fcx.demand_eq(func_span, func_ty, expected);
            ret_ty
        }
        _ => {
            fcx.report_type_error(
                func_span,
                TypeError::Custom(format!("expected function, found `{:?}`", func_ty)),
            );
            fcx.mk_error()
        }
    }
}

// ---------------------------------------------------------------------------
// Method call checking
// ---------------------------------------------------------------------------

fn check_method_call(
    fcx: &mut FnCtxt<'_>,
    receiver: ExprId,
    method: &yelang_ast::Ident,
    args: &[ExprId],
) -> TyId {
    if let Some(ty) = crate::array_builtins::try_check_array_method_call(fcx, receiver, method.symbol, args) {
        return ty;
    }
    crate::method::check_method_call(fcx, receiver, method.symbol, args)
}

// ---------------------------------------------------------------------------
// Field access checking
// ---------------------------------------------------------------------------

fn check_field(fcx: &mut FnCtxt<'_>, expr: ExprId, field: &yelang_ast::Ident) -> TyId {
    let expr_span = expr_span(fcx, expr);
    let expr_ty = check_expr(fcx, expr);
    let steps = crate::autoderef::probe_deref_steps(fcx, expr_ty);

    for (probe_ty, adjustments) in steps {
        if let Some(field_ty) = lookup_field(fcx, probe_ty, field) {
            // Commit to this deref chain. Built-in deref steps need no extra
            // work; user-defined `Deref` steps must be proven as obligations.
            for adj in &adjustments {
                if let crate::autoderef::Adjustment::DerefTrait { source, target } = *adj {
                    crate::autoderef::emit_deref_trait_obligations(fcx, source, target);
                }
            }
            if !adjustments.is_empty() {
                fcx.results.expr_adjustments.insert(expr, adjustments);
            }
            return field_ty;
        }
    }

    fcx.report_type_error(
        expr_span,
        TypeError::NoSuchField {
            ty: expr_ty,
            field: field.symbol,
        },
    );
    fcx.mk_error()
}

/// Look up a named field in a type (without considering deref).
fn lookup_field(fcx: &mut FnCtxt<'_>, ty: TyId, field: &yelang_ast::Ident) -> Option<TyId> {
    let interner = fcx.tcx.interner();

    match interner.ty(ty) {
        Ty::Tuple(args) => {
            let index = field.symbol.as_usize();
            args.get(index).and_then(|arg| match arg {
                GenericArg::Type(t) => Some(*t),
                _ => None,
            })
        }
        Ty::Adt(AdtDef { def_id }, args) => {
            let adt = fcx.tcx.adt_def(def_id)?;
            // For structs we only have a single variant; enums require match.
            let variant = adt.variants.first()?;
            let field_data = variant
                .fields
                .iter()
                .find(|f| f.ident.symbol == field.symbol)?;
            if args.is_empty() {
                Some(field_data.ty)
            } else {
                let subst = Substitution::from_args(args.iter().copied().collect());
                Some(substitute(interner, field_data.ty, &subst))
            }
        }
        Ty::AnonStruct(AnonStructDef { fields }) => {
            fields.iter().find(|f| f.name == field.symbol).map(|f| f.ty)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Index checking
// ---------------------------------------------------------------------------

fn check_index(fcx: &mut FnCtxt<'_>, expr: ExprId, index: ExprId) -> TyId {
    let base_span = expr_span(fcx, expr);
    let index_span = expr_span(fcx, index);
    let expr_ty = check_expr(fcx, expr);
    let index_ty = check_expr(fcx, index);
    let interner = fcx.tcx.interner();

    // Index must be integer
    fcx.demand_eq(index_span, fcx.mk_int(IntTy::I32), index_ty);

    match interner.ty(expr_ty) {
        Ty::Array(ty, _) | Ty::Slice(ty) => ty,
        Ty::Infer(InferTy::TyVar(_)) => {
            let elem_ty = fcx.new_ty_var();
            let expected = fcx.mk_slice(elem_ty);
            fcx.demand_eq(base_span, expr_ty, expected);
            elem_ty
        }
        _ => {
            fcx.report_type_error(
                base_span,
                TypeError::Custom(format!("cannot index a value of type `{:?}`", expr_ty)),
            );
            fcx.mk_error()
        }
    }
}

// ---------------------------------------------------------------------------
// Assignment checking
// ---------------------------------------------------------------------------

fn check_assign(fcx: &mut FnCtxt<'_>, left: ExprId, right: ExprId) -> TyId {
    let left_span = expr_span(fcx, left);
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);
    fcx.demand_eq(left_span, left_ty, right_ty);
    fcx.mk_unit()
}

// ---------------------------------------------------------------------------
// Block checking
// ---------------------------------------------------------------------------

fn check_block(fcx: &mut FnCtxt<'_>, block: &Block) -> TyId {
    fcx.push_scope();

    for stmt in &block.stmts {
        check_stmt(fcx, *stmt);
    }

    let ty = if let Some(expr) = &block.expr {
        check_expr(fcx, *expr)
    } else {
        fcx.mk_unit()
    };

    fcx.pop_scope();
    ty
}

fn check_stmt(fcx: &mut FnCtxt<'_>, stmt_id: StmtId) {
    let stmt = fcx
        .tcx
        .crate_hir()
        .stmt(stmt_id)
        .expect("StmtId should be valid")
        .clone();
    match &stmt {
        Stmt::Expr { expr } => {
            let _ = check_expr(fcx, *expr);
        }
        Stmt::Let { pat, ty, init } => {
            let init_ty = if let Some(init_expr) = init {
                check_expr(fcx, *init_expr)
            } else {
                fcx.new_ty_var()
            };

            let expected_ty = if let Some(hir_ty) = ty {
                let annotated = lower_hir_ty_id(fcx, *hir_ty);
                let init_span = init
                    .map(|e| expr_span(fcx, e))
                    .unwrap_or_else(yelang_lexer::Span::default);
                if init.is_some() {
                    if let Err(()) = fcx.coerce(init_ty, annotated) {
                        fcx.report_mismatch(init_span, annotated, init_ty);
                    }
                }
                annotated
            } else {
                init_ty
            };

            check_pat(fcx, *pat, expected_ty);
        }
        Stmt::Item { .. } => {
            // Nested items are checked separately
        }
    }
}

// ---------------------------------------------------------------------------
// Loop checking
// ---------------------------------------------------------------------------

fn check_loop(fcx: &mut FnCtxt<'_>, block: &Block, label: Option<&yelang_ast::Label>) -> TyId {
    fcx.push_breakable(BreakableScope {
        label: label.cloned(),
        kind: BreakableKind::Loop,
        expr_ty: fcx.mk_never(),
        span: block.span,
    });

    let _ = check_block(fcx, block);

    let scope = fcx.pop_breakable().unwrap();

    // If no breaks with values, loop type is never (diverges)
    // If breaks with values, type is the value type
    if fcx.tcx.interner().ty(scope.expr_ty).is_never() {
        fcx.mk_never()
    } else {
        scope.expr_ty
    }
}

// ---------------------------------------------------------------------------
// Break checking
// ---------------------------------------------------------------------------

fn check_break(
    fcx: &mut FnCtxt<'_>,
    label: Option<&yelang_ast::Label>,
    expr: Option<ExprId>,
) -> TyId {
    let breakable_idx = if let Some(lbl) = label {
        fcx.breakable_scopes.iter().rposition(|s| {
            s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize())
        })
    } else {
        fcx.breakable_scopes
            .iter()
            .rposition(|s| s.kind == BreakableKind::Loop)
    };

    if let Some(idx) = breakable_idx {
        let _expr_ty = if let Some(e) = expr {
            let span = expr_span(fcx, e);
            let ty = check_expr(fcx, e);
            // We need to mutate the scope, so we can't hold a reference.
            let scope = &mut fcx.breakable_scopes[idx];
            if fcx.tcx.interner().ty(scope.expr_ty).is_never() {
                // First break: set the scope type
                scope.expr_ty = ty;
            } else {
                let scope_expr_ty = scope.expr_ty;
                fcx.demand_eq(span, scope_expr_ty, ty);
            }
            ty
        } else {
            let unit = fcx.mk_unit();
            let scope = &mut fcx.breakable_scopes[idx];
            if fcx.tcx.interner().ty(scope.expr_ty).is_never() {
                scope.expr_ty = unit;
            } else {
                let scope_expr_ty = scope.expr_ty;
                fcx.demand_eq(yelang_lexer::Span::default(), scope_expr_ty, unit);
            }
            unit
        };
    }

    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Continue checking
// ---------------------------------------------------------------------------

fn check_continue(fcx: &mut FnCtxt<'_>, label: Option<&yelang_ast::Label>) -> TyId {
    let _ = if let Some(lbl) = label {
        fcx.breakable_scopes
            .iter()
            .rev()
            .find(|s| s.label.as_ref().map(|l| l.symbol.as_usize()) == Some(lbl.symbol.as_usize()))
    } else {
        fcx.breakable_scopes
            .iter()
            .rev()
            .find(|s| s.kind == BreakableKind::Loop)
    };
    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Return checking
// ---------------------------------------------------------------------------

fn check_return(fcx: &mut FnCtxt<'_>, expr: Option<ExprId>) -> TyId {
    if let Some(e) = expr {
        let span = expr_span(fcx, e);
        let ty = check_expr(fcx, e);
        fcx.demand_eq(span, fcx.return_ty, ty);
    } else {
        fcx.demand_eq(yelang_lexer::Span::default(), fcx.return_ty, fcx.mk_unit());
    }

    fcx.mk_never()
}

// ---------------------------------------------------------------------------
// Match checking
// ---------------------------------------------------------------------------

fn check_match(fcx: &mut FnCtxt<'_>, expr: ExprId, arms: &[Arm]) -> TyId {
    let scrutinee_ty = check_expr(fcx, expr);
    let result_ty = fcx.new_ty_var();

    for arm in arms {
        check_pat(fcx, arm.pat, scrutinee_ty);
        if let Some(guard) = &arm.guard {
            let guard_span = expr_span(fcx, *guard);
            let guard_ty = check_expr(fcx, *guard);
            fcx.demand_eq(guard_span, guard_ty, fcx.mk_bool());
        }
        let body_span = expr_span(fcx, arm.body);
        let body_ty = check_expr(fcx, arm.body);
        fcx.demand_eq(body_span, result_ty, body_ty);
    }

    result_ty
}

// ---------------------------------------------------------------------------
// If checking
// ---------------------------------------------------------------------------

fn check_if(
    fcx: &mut FnCtxt<'_>,
    cond: ExprId,
    then_branch: ExprId,
    else_branch: Option<ExprId>,
) -> TyId {
    let cond_span = expr_span(fcx, cond);
    let cond_ty = check_expr(fcx, cond);
    fcx.demand_eq(cond_span, cond_ty, fcx.mk_bool());

    let then_ty = check_expr(fcx, then_branch);

    if let Some(else_expr) = else_branch {
        let else_span = expr_span(fcx, else_expr);
        let else_ty = check_expr(fcx, else_expr);
        fcx.demand_eq(else_span, then_ty, else_ty);
        then_ty
    } else {
        // No else branch: then branch must evaluate to unit
        let then_span = expr_span(fcx, then_branch);
        fcx.demand_eq(then_span, then_ty, fcx.mk_unit());
        fcx.mk_unit()
    }
}

// ---------------------------------------------------------------------------
// Let expression checking (for if let)
// ---------------------------------------------------------------------------

fn check_let_expr(fcx: &mut FnCtxt<'_>, pat: PatId, expr: ExprId) -> TyId {
    let expr_ty = check_expr(fcx, expr);
    check_pat(fcx, pat, expr_ty);
    fcx.mk_bool()
}

// ---------------------------------------------------------------------------
// Closure checking
// ---------------------------------------------------------------------------

fn check_closure(
    fcx: &mut FnCtxt<'_>,
    _params: &[yelang_hir::hir::body::Param],
    body_id: BodyId,
) -> TyId {
    check_closure_with_expected(fcx, body_id, &[])
}

/// Type-check a closure whose parameter types are partially or fully known from
/// the call context (e.g. `users.any(|u| u.id > 0)` where `u` must be `User`).
///
/// `expected_inputs[i]` is used for parameter `i` when the source does not
/// supply an explicit type annotation. Extra expected types beyond the number
/// of parameters are ignored; missing expectations fall back to fresh type
/// variables.
pub(crate) fn check_closure_with_expected(
    fcx: &mut FnCtxt<'_>,
    body_id: BodyId,
    expected_inputs: &[TyId],
) -> TyId {
    let body = match fcx.tcx.crate_hir().body(body_id) {
        Some(b) => b.clone(),
        None => return fcx.mk_error(),
    };

    fcx.push_scope();

    let mut param_tys = Vec::with_capacity(body.params.len());
    for (idx, param) in body.params.iter().enumerate() {
        let annotated = lower_hir_ty_id(fcx, param.ty);
        let annotated_is_inferred = fcx
            .tcx
            .crate_hir()
            .ty(param.ty)
            .is_some_and(|t| matches!(t, HirTy::Infer | HirTy::Missing | HirTy::Err));
        let param_ty = if annotated_is_inferred {
            expected_inputs.get(idx).copied().unwrap_or_else(|| fcx.new_ty_var())
        } else {
            annotated
        };
        param_tys.push(param_ty);
        crate::pat::check_pat(fcx, param.pat, param_ty);
    }

    let output_ty = check_expr(fcx, body.value);

    fcx.pop_scope();

    let inputs = fcx
        .tcx
        .interner()
        .mk_generic_args(&param_tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>());
    fcx.mk_fn_ptr(inputs, output_ty)
}

// ---------------------------------------------------------------------------
// Struct literal checking
// ---------------------------------------------------------------------------

fn check_struct_literal(
    fcx: &mut FnCtxt<'_>,
    path: &Res,
    fields: &[FieldExpr],
    rest: Option<ExprId>,
) -> TyId {
    let struct_ty = check_path(fcx, path);

    for field in fields {
        let _field_ty = check_expr(fcx, field.expr);
        // TODO: check field type against struct definition
    }

    if let Some(rest_expr) = rest {
        let _ = check_expr(fcx, rest_expr);
    }

    struct_ty
}

// ---------------------------------------------------------------------------
// Tuple checking
// ---------------------------------------------------------------------------

fn check_tuple(fcx: &mut FnCtxt<'_>, exprs: &[ExprId]) -> TyId {
    let tys: Vec<_> = exprs.iter().map(|e| check_expr(fcx, *e)).collect();
    let args = fcx
        .tcx
        .interner()
        .mk_generic_args(&tys.iter().map(|&t| GenericArg::Type(t)).collect::<Vec<_>>());
    fcx.mk_ty(Ty::Tuple(args))
}

// ---------------------------------------------------------------------------
// Array checking
// ---------------------------------------------------------------------------

fn check_array(fcx: &mut FnCtxt<'_>, exprs: &[ExprId]) -> TyId {
    let elem_ty = if exprs.is_empty() {
        fcx.new_ty_var()
    } else {
        let first_ty = check_expr(fcx, exprs[0]);
        for expr in exprs.iter().skip(1) {
            let span = expr_span(fcx, *expr);
            let ty = check_expr(fcx, *expr);
            fcx.demand_eq(span, first_ty, ty);
        }
        first_ty
    };

    // Dynamic arrays are represented by the prelude `Array<T>` lang item. If it
    // is unavailable (e.g. in isolated unit tests), fall back to the fixed-size
    // array type so that type checking still has a representation to work with.
    if fcx.tcx.lang_item(yelang_resolve::lang_items::LangItem::Array).is_some() {
        fcx.mk_array_ty(elem_ty)
    } else {
        let len = fcx.tcx.interner().mk_const_from_parts(
            yelang_ty::ty::Const::Value(yelang_ty::ty::ConstValue::Int(exprs.len() as i128)),
            fcx.mk_int(IntTy::I32),
        );
        fcx.mk_array(elem_ty, len)
    }
}

// ---------------------------------------------------------------------------
// Cast checking
// ---------------------------------------------------------------------------

fn check_cast(fcx: &mut FnCtxt<'_>, expr: ExprId, ty: HirTyId) -> TyId {
    let _expr_ty = check_expr(fcx, expr);
    lower_hir_ty_id(fcx, ty)
}

// ---------------------------------------------------------------------------
// HIR type lowering helper
// ---------------------------------------------------------------------------

fn lower_hir_ty_id(fcx: &mut FnCtxt<'_>, ty_id: HirTyId) -> TyId {
    let hir_ty = fcx
        .tcx
        .crate_hir()
        .ty(ty_id)
        .expect("TyId should be valid")
        .clone();
    lower_hir_ty(&hir_ty, fcx)
}

// ---------------------------------------------------------------------------
// Assign-op, range, object, try
// ---------------------------------------------------------------------------

fn check_assign_op(fcx: &mut FnCtxt<'_>, op: AssignOpKind, left: ExprId, right: ExprId) -> TyId {
    let bin_op = assign_op_to_bin_op(op);
    let left_ty = check_expr(fcx, left);
    let right_ty = check_expr(fcx, right);

    // Assignment operators only map to arithmetic and bitwise binary ops.
    // The result of the underlying operation must be assignable back to `left`.
    match bin_op {
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Divide
        | BinaryOp::Modulo
        | BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr => {
            fcx.demand_eq(expr_span(fcx, right), left_ty, right_ty);
        }
        _ => unreachable!("assignment operators cannot map to {bin_op:?}"),
    }

    fcx.mk_unit()
}

fn assign_op_to_bin_op(op: AssignOpKind) -> BinaryOp {
    use yelang_ast::BinaryOp;
    match op {
        AssignOpKind::AddEq => BinaryOp::Add,
        AssignOpKind::SubEq => BinaryOp::Subtract,
        AssignOpKind::MulEq => BinaryOp::Multiply,
        AssignOpKind::DivEq => BinaryOp::Divide,
        AssignOpKind::ModEq => BinaryOp::Modulo,
        AssignOpKind::BitAndEq => BinaryOp::BitAnd,
        AssignOpKind::BitOrEq => BinaryOp::BitOr,
        AssignOpKind::BitXorEq => BinaryOp::BitXor,
        AssignOpKind::BitShlEq => BinaryOp::Shl,
        AssignOpKind::BitShrEq => BinaryOp::Shr,
    }
}

fn check_range(fcx: &mut FnCtxt<'_>, start: Option<ExprId>, end: Option<ExprId>) -> TyId {
    if let Some(e) = start {
        let _ = check_expr(fcx, e);
    }
    if let Some(e) = end {
        let _ = check_expr(fcx, e);
    }
    // Range type is language-defined; return a fresh variable until the
    // standard library Range type is wired up.
    fcx.new_ty_var()
}

fn check_object_literal(fcx: &mut FnCtxt<'_>, fields: &[FieldExpr]) -> TyId {
    for field in fields {
        let _ = check_expr(fcx, field.expr);
    }
    fcx.new_ty_var()
}

fn check_try(fcx: &mut FnCtxt<'_>, expr: ExprId) -> TyId {
    let inner_ty = check_expr(fcx, expr);
    // `expr?` unwraps the inner Ok/Some value. Until Result is modeled,
    // return the inner type and let inference sort it out.
    inner_ty
}
