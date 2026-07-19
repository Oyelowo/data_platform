//! Bootstrap construction of `Aggregate` trait closures for built-in markers.
//!
//! Until the compiler type-checks impl bodies, this module builds the
//! `init`/`step`/`merge`/`finish`/`config` QExpr closures for the built-in
//! aggregate markers (`Sum`, `Product`, `Count`, `Avg`) directly.  User-defined
//! aggregates will be handled by extracting their bodies from typed HIR once
//! impl-body typechecking is implemented.

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_ty::ty::{Ty, TyId};

use crate::expr::{CastKind, QBinaryOp, QExpr, QExprId, QLit};
use crate::ids::BinderId;
use crate::logical::lower::LoweringCtxt;
use crate::logical::operator::AggregateOp;
use crate::logical::plan::LogicalPlan;

/// Known built-in aggregate marker names.
const SUM: &str = "Sum";
const PRODUCT: &str = "Product";
const COUNT: &str = "Count";
const AVG: &str = "Avg";

// Bootstrap note: accumulator field symbols are allocated from a high raw-id
// range so they do not collide with symbols produced by ordinary source code.
// Once impl bodies are type-checked these closures will be extracted from the
// actual `.ye` stdlib and these constants will disappear.
fn value_field() -> Symbol { Symbol::from(1000) }
fn sum_field() -> Symbol { Symbol::from(1001) }
fn count_field() -> Symbol { Symbol::from(1002) }

/// Try to build a bootstrap `AggregateOp` for a built-in marker.
///
/// Returns `None` if `agg_def` is not a built-in marker we know how to
/// synthesize, or if its name cannot be resolved.  The caller supplies the
/// `per_row` expression, the classification (read from the `.ye` stdlib), and
/// the element/output types from type checking.
/// Resolve a `Queryable` sugar method (e.g. `sum`) to its aggregate marker
/// type `DefId` (e.g. `Sum`).  This is a bootstrap helper: once impl bodies are
/// type-checked we will lower sugar by inlining the trait default body instead.
pub fn resolve_sugar_marker(ctx: &LoweringCtxt<'_>, method_def_id: DefId) -> Option<DefId> {
    let item = ctx.krate().items.get(method_def_id).and_then(|i| i.as_ref())?;
    let name = ctx.tcx.resolve_symbol(item.ident.symbol)?;
    let marker_name = match name {
        "sum" => SUM,
        "product" => PRODUCT,
        "count" => COUNT,
        "avg" => AVG,
        "min" => "Min",
        "max" => "Max",
        _ => return None,
    };
    find_item_def_by_name(ctx, marker_name)
}

fn find_item_def_by_name(ctx: &LoweringCtxt<'_>, name: &str) -> Option<DefId> {
    for (def_id, item) in ctx.krate().items.iter_enumerated() {
        let Some(item) = item.as_ref() else { continue };
        let Some(item_name) = ctx.tcx.resolve_symbol(item.ident.symbol) else { continue };
        if item_name == name {
            return Some(def_id);
        }
    }
    None
}

pub fn build_builtin_aggregate(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    agg_def: DefId,
    per_row: QExprId,
    class: crate::expr::AggregateClass,
    elem_ty: TyId,
    out_ty: TyId,
) -> Option<AggregateOp> {
    let name = aggregate_name(ctx, agg_def)?;
    match name.as_str() {
        SUM => Some(build_sum(plan, ctx, agg_def, per_row, class, elem_ty, out_ty)),
        PRODUCT => Some(build_product(plan, ctx, agg_def, per_row, class, elem_ty, out_ty)),
        COUNT => Some(build_count(plan, ctx, agg_def, per_row, class, elem_ty, out_ty)),
        AVG => Some(build_avg(plan, ctx, agg_def, per_row, class, elem_ty, out_ty)),
        _ => None,
    }
}

fn aggregate_name(ctx: &LoweringCtxt<'_>, agg_def: DefId) -> Option<String> {
    let item = ctx.krate().items.get(agg_def).and_then(|i| i.as_ref())?;
    let name = ctx.tcx.resolve_symbol(item.ident.symbol)?;
    Some(name.to_string())
}

fn is_float(ctx: &LoweringCtxt<'_>, ty: TyId) -> bool {
    let interner = ctx.tcx.interner();
    if !interner.has_ty(ty) {
        return false;
    }
    matches!(interner.ty(ty), Ty::Float(_))
}

fn build_sum(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    agg_def: DefId,
    per_row: QExprId,
    class: crate::expr::AggregateClass,
    elem_ty: TyId,
    out_ty: TyId,
) -> AggregateOp {
    let value = value_field();
    let zero = lit(plan, if is_float(ctx, elem_ty) { QLit::Float(0.0) } else { QLit::Int(0) }, elem_ty);
    let init_body = record(plan, vec![(value, zero)], elem_ty);
    let init = closure0(plan, init_body);

    let (step, merge) = value_accum_closures(plan, ctx, elem_ty, value, QBinaryOp::Add);
    let finish = value_finish_closure(plan, elem_ty, value, out_ty);
    let config = unit_config(plan, elem_ty);

    AggregateOp {
        agg_def,
        impl_def: agg_def,
        class,
        per_row,
        init,
        step,
        merge,
        finish,
        config,
        acc_ty: elem_ty,
        out_ty,
    }
}

fn build_product(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    agg_def: DefId,
    per_row: QExprId,
    class: crate::expr::AggregateClass,
    elem_ty: TyId,
    out_ty: TyId,
) -> AggregateOp {
    let value = value_field();
    let one = lit(plan, if is_float(ctx, elem_ty) { QLit::Float(1.0) } else { QLit::Int(1) }, elem_ty);
    let init_body = record(plan, vec![(value, one)], elem_ty);
    let init = closure0(plan, init_body);

    let (step, merge) = value_accum_closures(plan, ctx, elem_ty, value, QBinaryOp::Mul);
    let finish = value_finish_closure(plan, elem_ty, value, out_ty);
    let config = unit_config(plan, elem_ty);

    AggregateOp {
        agg_def,
        impl_def: agg_def,
        class,
        per_row,
        init,
        step,
        merge,
        finish,
        config,
        acc_ty: elem_ty,
        out_ty,
    }
}

fn build_count(
    plan: &mut LogicalPlan,
    _ctx: &LoweringCtxt<'_>,
    agg_def: DefId,
    per_row: QExprId,
    class: crate::expr::AggregateClass,
    elem_ty: TyId,
    out_ty: TyId,
) -> AggregateOp {
    let count = count_field();
    let zero = lit(plan, QLit::Int(0), elem_ty);
    let init_body = record(plan, vec![(count, zero)], elem_ty);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc, elem_ty);
    let count_field = field(plan, acc_col, count, elem_ty);
    let one = lit(plan, QLit::Int(1), elem_ty);
    let new_count = binary(plan, QBinaryOp::Add, count_field, one, elem_ty);
    let step_body = record(plan, vec![(count, new_count)], elem_ty);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a, elem_ty);
    let b_col = col(plan, b, elem_ty);
    let a_count = field(plan, a_col, count, elem_ty);
    let b_count = field(plan, b_col, count, elem_ty);
    let merged = binary(plan, QBinaryOp::Add, a_count, b_count, elem_ty);
    let merge_body = record(plan, vec![(count, merged)], elem_ty);
    let merge = closure2(plan, a, b, merge_body);

    let acc = plan.fresh_binder();
    let acc_col2 = col(plan, acc, elem_ty);
    let finish_body = field(plan, acc_col2, count, out_ty);
    let finish = closure1(plan, acc, finish_body);

    let config = unit_config(plan, elem_ty);

    AggregateOp {
        agg_def,
        impl_def: agg_def,
        class,
        per_row,
        init,
        step,
        merge,
        finish,
        config,
        acc_ty: elem_ty,
        out_ty,
    }
}

fn build_avg(
    plan: &mut LogicalPlan,
    ctx: &LoweringCtxt<'_>,
    agg_def: DefId,
    per_row: QExprId,
    class: crate::expr::AggregateClass,
    elem_ty: TyId,
    out_ty: TyId,
) -> AggregateOp {
    let sum = sum_field();
    let count = count_field();
    let zero = lit(plan, if is_float(ctx, elem_ty) { QLit::Float(0.0) } else { QLit::Int(0) }, elem_ty);
    let zero_count = lit(plan, QLit::Int(0), elem_ty);
    let init_body = record(plan, vec![(sum, zero), (count, zero_count)], elem_ty);
    let init = closure0(plan, init_body);

    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc, elem_ty);
    let item_col = col(plan, item, elem_ty);
    let sum_field = field(plan, acc_col, sum, elem_ty);
    let count_field = field(plan, acc_col, count, elem_ty);
    let new_sum = binary(plan, QBinaryOp::Add, sum_field, item_col, elem_ty);
    let one = lit(plan, QLit::Int(1), elem_ty);
    let new_count = binary(plan, QBinaryOp::Add, count_field, one, elem_ty);
    let step_body = record(plan, vec![(sum, new_sum), (count, new_count)], elem_ty);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a, elem_ty);
    let b_col = col(plan, b, elem_ty);
    let a_sum = field(plan, a_col, sum, elem_ty);
    let b_sum = field(plan, b_col, sum, elem_ty);
    let a_count = field(plan, a_col, count, elem_ty);
    let b_count = field(plan, b_col, count, elem_ty);
    let merged_sum = binary(plan, QBinaryOp::Add, a_sum, b_sum, elem_ty);
    let merged_count = binary(plan, QBinaryOp::Add, a_count, b_count, elem_ty);
    let merge_body = record(plan, vec![(sum, merged_sum), (count, merged_count)], elem_ty);
    let merge = closure2(plan, a, b, merge_body);

    let acc = plan.fresh_binder();
    let acc_col2 = col(plan, acc, elem_ty);
    let sum_field = field(plan, acc_col2, sum, elem_ty);
    let count_field = field(plan, acc_col2, count, elem_ty);
    let sum_as_float = cast(plan, sum_field, CastKind::IntToFloat, out_ty);
    let count_as_float = cast(plan, count_field, CastKind::IntToFloat, out_ty);
    let finish_body = binary(plan, QBinaryOp::Div, sum_as_float, count_as_float, out_ty);
    let finish = closure1(plan, acc, finish_body);

    let config = unit_config(plan, elem_ty);

    AggregateOp {
        agg_def,
        impl_def: agg_def,
        class,
        per_row,
        init,
        step,
        merge,
        finish,
        config,
        acc_ty: elem_ty,
        out_ty,
    }
}

/// Build `(acc, item) -> { value: acc.value op item }` and
/// `(a, b) -> { value: a.value op b.value }` closures for a single-field
/// numeric accumulator.
fn value_accum_closures(
    plan: &mut LogicalPlan,
    _ctx: &LoweringCtxt<'_>,
    elem_ty: TyId,
    field_sym: Symbol,
    op: QBinaryOp,
) -> (QExprId, QExprId) {
    let acc = plan.fresh_binder();
    let item = plan.fresh_binder();
    let acc_col = col(plan, acc, elem_ty);
    let item_col = col(plan, item, elem_ty);
    let acc_value = field(plan, acc_col, field_sym, elem_ty);
    let new_value = binary(plan, op, acc_value, item_col, elem_ty);
    let step_body = record(plan, vec![(field_sym, new_value)], elem_ty);
    let step = closure2(plan, acc, item, step_body);

    let a = plan.fresh_binder();
    let b = plan.fresh_binder();
    let a_col = col(plan, a, elem_ty);
    let b_col = col(plan, b, elem_ty);
    let a_value = field(plan, a_col, field_sym, elem_ty);
    let b_value = field(plan, b_col, field_sym, elem_ty);
    let merged = binary(plan, op, a_value, b_value, elem_ty);
    let merge_body = record(plan, vec![(field_sym, merged)], elem_ty);
    let merge = closure2(plan, a, b, merge_body);

    (step, merge)
}

fn value_finish_closure(
    plan: &mut LogicalPlan,
    elem_ty: TyId,
    field_sym: Symbol,
    out_ty: TyId,
) -> QExprId {
    let acc = plan.fresh_binder();
    let acc_col = col(plan, acc, elem_ty);
    let body = field(plan, acc_col, field_sym, out_ty);
    closure1(plan, acc, body)
}

fn unit_config(plan: &mut LogicalPlan, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Record(vec![], ty))
}

fn lit(plan: &mut LogicalPlan, value: QLit, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Lit(value, ty))
}

fn col(plan: &mut LogicalPlan, binder: BinderId, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Column(binder, ty))
}

fn field(plan: &mut LogicalPlan, record: QExprId, field: Symbol, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Field(record, field, ty))
}

fn record(plan: &mut LogicalPlan, fields: Vec<(Symbol, QExprId)>, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Record(fields, ty))
}

fn binary(plan: &mut LogicalPlan, op: QBinaryOp, left: QExprId, right: QExprId, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Binary(op, left, right, ty))
}

fn cast(plan: &mut LogicalPlan, expr: QExprId, kind: CastKind, ty: TyId) -> QExprId {
    plan.alloc_expr(QExpr::Cast(expr, kind, ty))
}

fn closure0(plan: &mut LogicalPlan, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![],
        body,
        captures: vec![],
        ty: body_ty(plan, body),
    })
}

fn closure1(plan: &mut LogicalPlan, param: BinderId, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![param],
        body,
        captures: vec![],
        ty: body_ty(plan, body),
    })
}

fn closure2(plan: &mut LogicalPlan, p1: BinderId, p2: BinderId, body: QExprId) -> QExprId {
    plan.alloc_expr(QExpr::Closure {
        params: vec![p1, p2],
        body,
        captures: vec![],
        ty: body_ty(plan, body),
    })
}

fn body_ty(plan: &LogicalPlan, body: QExprId) -> TyId {
    plan.expr(body).ty()
}
