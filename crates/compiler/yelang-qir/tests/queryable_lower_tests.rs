//! Exhaustive lowering tests for `Queryable`/`Aggregate`/`Iterator` method calls.
//!
//! These tests construct typed HIR by hand, seed synthetic lang items and
//! method resolutions, and verify that the LIR operators match the expected
//! shape. No method-name matching is involved.

use yelang_arena::DefId;
use yelang_hir::hir::body::{Body, Param};
use yelang_hir::hir::core::CaptureClause;
use yelang_hir::hir::expr::Expr;
use yelang_hir::res::Res;
use yelang_hir::hir::pat::Pat;
use yelang_hir::ids::{BodyId, ExprId, PatId};
use yelang_hir::Crate;
use yelang_interner::Symbol;
use yelang_lexer::{Ident, Literal, Span};
use yelang_qir::errors::LoweringError;
use yelang_qir::expr::{AggregateClass, QExpr};
use yelang_qir::ids::LirId;
use yelang_qir::logical::lower::LoweringCtxt;
use yelang_qir::logical::operator::LirOp;
use yelang_qir::logical::plan::LogicalPlan;
use yelang_qir::logical::queryable::QueryableMethod;
use yelang_resolve::lang_items::LangItem;
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::typeck_results::{MethodResolution, TypeckResults};

fn dummy_ty() -> yelang_ty::ty::TyId {
    yelang_ty::ty::TyId::new(1)
}

fn ident(name: &str) -> Ident {
    let symbol = Symbol::from(name.as_bytes()[0] as u32);
    Ident::new(symbol, Span::default())
}

fn mk_tcx() -> TyCtxt {
    let mut krate = Crate::new(DefId::new(1));
    krate.lang_items.insert(LangItem::Queryable, DefId::new(100));
    krate.lang_items.insert(LangItem::Aggregate, DefId::new(101));
    krate.lang_items.insert(LangItem::Iterator, DefId::new(102));
    krate.lang_items.insert(LangItem::IntoIterator, DefId::new(103));
    TyCtxt::with_string_interner(krate, yelang_interner::Interner::new())
}

fn queryable_def(tcx: &TyCtxt) -> DefId {
    tcx.lang_item(LangItem::Queryable).unwrap()
}

fn aggregate_def(tcx: &TyCtxt) -> DefId {
    tcx.lang_item(LangItem::Aggregate).unwrap()
}

fn iterator_def(tcx: &TyCtxt) -> DefId {
    tcx.lang_item(LangItem::Iterator).unwrap()
}

fn into_iter_def(tcx: &TyCtxt) -> DefId {
    tcx.lang_item(LangItem::IntoIterator).unwrap()
}

fn lit_expr(tcx: &mut TyCtxt, value: i128) -> ExprId {
    let s = format!("{}", value);
    let sym = tcx.intern_symbol(&s).unwrap();
    tcx.crate_hir_mut().alloc_expr(
        Expr::Lit {
            lit: Literal::Int(yelang_lexer::IntegerLit {
                value: sym,
                suffix: None,
            }),
        },
        Span::default(),
    )
}

fn bool_lit_expr(tcx: &mut TyCtxt, value: bool) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(
        Expr::Lit {
            lit: Literal::Bool(value),
        },
        Span::default(),
    )
}

fn array_expr(tcx: &mut TyCtxt, elems: Vec<ExprId>) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(Expr::Array { exprs: elems }, Span::default())
}

fn array_of_ints(tcx: &mut TyCtxt, values: &[i128]) -> ExprId {
    let elems: Vec<ExprId> = values.iter().map(|&v| lit_expr(tcx, v)).collect();
    array_expr(tcx, elems)
}

fn param_pat(tcx: &mut TyCtxt) -> PatId {
    tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default())
}

fn closure_expr(tcx: &mut TyCtxt, param: PatId, body: ExprId) -> ExprId {
    let body_id = tcx.crate_hir_mut().alloc_body(
        Body {
            params: vec![Param {
                pat: param,
                ty: yelang_hir::ids::HirTyId::default(),
                span: Span::default(),
            }],
            value: body,
            span: Span::default(),
        },
        Span::default(),
    );
    tcx.crate_hir_mut().alloc_expr(
        Expr::Closure {
            params: vec![Param {
                pat: param,
                ty: yelang_hir::ids::HirTyId::default(),
                span: Span::default(),
            }],
            body: body_id,
            capture_clause: CaptureClause::Ref,
        },
        Span::default(),
    )
}

fn method_call_expr(
    tcx: &mut TyCtxt,
    receiver: ExprId,
    method_name: &str,
    args: Vec<ExprId>,
) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(
        Expr::MethodCall {
            receiver,
            method: ident(method_name),
            args,
            trait_def_id: None,
        },
        Span::default(),
    )
}

fn field_expr(tcx: &mut TyCtxt, base: ExprId, field: &str) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(
        Expr::Field {
            expr: base,
            field: ident(field),
        },
        Span::default(),
    )
}

fn path_local_expr(tcx: &mut TyCtxt, pat_id: PatId) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Local { pat_id },
        },
        Span::default(),
    )
}

fn comprehension_expr(
    tcx: &mut TyCtxt,
    element: ExprId,
    pat: PatId,
    source: ExprId,
    condition: Option<ExprId>,
) -> ExprId {
    tcx.crate_hir_mut().alloc_expr(
        Expr::Comprehension {
            kind: yelang_hir::hir::expr::ComprehensionKind::List,
            element,
            variables: vec![yelang_hir::hir::expr::ComprehensionVar {
                pat,
                source,
                flatten: 0,
            }],
            condition,
        },
        Span::default(),
    )
}

fn record_method_resolution(
    results: &mut TypeckResults,
    expr_id: ExprId,
    trait_def_id: DefId,
    method_def_id: DefId,
) {
    results.record_method_resolution(
        expr_id,
        MethodResolution {
            trait_def_id: Some(trait_def_id),
            method_def_id: Some(method_def_id),
            impl_def_id: None,
        },
    );
}

fn lower_single_expr(
    tcx: &mut TyCtxt,
    expr_id: ExprId,
    results: &mut TypeckResults,
    queryable_methods: &[(DefId, QueryableMethod)],
    aggregate_classes: &[(DefId, AggregateClass)],
) -> (LogicalPlan, yelang_qir::ids::QExprId) {
    results.expr_types.insert(expr_id, dummy_ty());
    try_lower_single_expr(tcx, expr_id, results, queryable_methods, aggregate_classes)
        .expect("lowering should succeed")
}

fn lower_single_expr_without_type(
    tcx: &mut TyCtxt,
    expr_id: ExprId,
    results: &mut TypeckResults,
    queryable_methods: &[(DefId, QueryableMethod)],
    aggregate_classes: &[(DefId, AggregateClass)],
) -> (LogicalPlan, yelang_qir::ids::QExprId) {
    try_lower_single_expr(tcx, expr_id, results, queryable_methods, aggregate_classes)
        .expect("lowering should succeed")
}

fn try_lower_single_expr(
    tcx: &mut TyCtxt,
    expr_id: ExprId,
    results: &mut TypeckResults,
    queryable_methods: &[(DefId, QueryableMethod)],
    aggregate_classes: &[(DefId, AggregateClass)],
) -> Result<(LogicalPlan, yelang_qir::ids::QExprId), yelang_qir::errors::LoweringError> {
    let mut ctx = LoweringCtxt::new(tcx, BodyId::default(), results)
        .with_queryable_method(DefId::new(200), QueryableMethod::Filter)
        .with_queryable_method(DefId::new(201), QueryableMethod::Map)
        .with_queryable_method(DefId::new(202), QueryableMethod::FlatMap)
        .with_queryable_method(DefId::new(203), QueryableMethod::Take)
        .with_queryable_method(DefId::new(204), QueryableMethod::Skip)
        .with_queryable_method(DefId::new(205), QueryableMethod::OrderBy)
        .with_queryable_method(DefId::new(206), QueryableMethod::Distinct)
        .with_queryable_method(DefId::new(207), QueryableMethod::GroupBy)
        .with_queryable_method(DefId::new(208), QueryableMethod::Aggregate)
        .with_queryable_method(DefId::new(209), QueryableMethod::Sum)
        .with_queryable_method(DefId::new(210), QueryableMethod::Avg)
        .with_queryable_method(DefId::new(211), QueryableMethod::Count)
        .with_queryable_method(DefId::new(212), QueryableMethod::Execute)
        .with_aggregate_class(DefId::new(209), AggregateClass::Distributive)
        .with_aggregate_class(DefId::new(210), AggregateClass::Algebraic)
        .with_aggregate_class(DefId::new(211), AggregateClass::Distributive)
        .with_aggregate_class(DefId::new(300), AggregateClass::Distributive);

    for (def, method) in queryable_methods {
        ctx.queryable_methods.insert(*def, *method);
    }
    for (def, class) in aggregate_classes {
        ctx.aggregate_classes.insert(*def, *class);
    }

    let mut plan = LogicalPlan::empty();
    let qexpr = yelang_qir::logical::lower_expr::lower_hir_expr(&mut plan, &mut ctx, expr_id)?;
    Ok((plan, qexpr))
}

fn unwrap_subplan(plan: &LogicalPlan, qexpr: yelang_qir::ids::QExprId) -> LirId {
    match plan.expr(qexpr) {
        QExpr::Subplan(lir, _) => *lir,
        other => panic!("expected Subplan, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Individual operator lowering
// -----------------------------------------------------------------------------

#[test]
fn queryable_filter_lowers_to_filter() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1, 2]);
    let pred_body = bool_lit_expr(&mut tcx, true);
    let param = param_pat(&mut tcx);
    let closure = closure_expr(&mut tcx, param, pred_body);
    let call = method_call_expr(&mut tcx, src, "filter", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(200));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(pred_body, dummy_ty());
    results.expr_types.insert(lit_expr(&mut tcx, 1), dummy_ty());
    results.expr_types.insert(lit_expr(&mut tcx, 2), dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Filter { .. }));
}

#[test]
fn queryable_map_lowers_to_map() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let proj_body = lit_expr(&mut tcx, 42);
    let closure = closure_expr(&mut tcx, param, proj_body);
    let call = method_call_expr(&mut tcx, src, "map", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(201));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(proj_body, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
}

#[test]
fn queryable_flat_map_lowers_to_flat_map() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let proj_body = array_of_ints(&mut tcx, &[1]);
    let closure = closure_expr(&mut tcx, param, proj_body);
    let call = method_call_expr(&mut tcx, src, "flat_map", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(202));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(proj_body, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::FlatMap { .. }));
}

#[test]
fn queryable_take_lowers_to_slice() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let n = lit_expr(&mut tcx, 10);
    let call = method_call_expr(&mut tcx, src, "take", vec![n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(203));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Slice { .. }));
}

#[test]
fn queryable_skip_lowers_to_slice() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let n = lit_expr(&mut tcx, 5);
    let call = method_call_expr(&mut tcx, src, "skip", vec![n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(204));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Slice { .. }));
}

#[test]
fn queryable_order_by_lowers_to_order_by() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let key_body = lit_expr(&mut tcx, 1);
    let closure = closure_expr(&mut tcx, param, key_body);
    let call = method_call_expr(&mut tcx, src, "order_by", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(205));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(key_body, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::OrderBy { .. }));
}

#[test]
fn queryable_distinct_lowers_to_distinct() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "distinct", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(206));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Distinct { .. }));
}

#[test]
fn queryable_group_by_lowers_to_group_by() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let key_body = lit_expr(&mut tcx, 1);
    let closure = closure_expr(&mut tcx, param, key_body);
    let call = method_call_expr(&mut tcx, src, "group_by", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(207));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(key_body, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::GroupBy { .. }));
}

#[test]
fn queryable_aggregate_with_marker_lowers_to_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(300) },
        },
        Span::default(),
    );
    let call = method_call_expr(&mut tcx, src, "aggregate", vec![marker]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(208));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(marker, dummy_ty());

    let (plan, qexpr) = lower_single_expr(
        &mut tcx,
        call,
        &mut results,
        &[],
        &[(DefId::new(300), AggregateClass::Distributive)],
    );
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Aggregate { .. }));
}

#[test]
fn queryable_sum_sugar_lowers_to_distributive_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "sum", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(209));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => assert_eq!(agg.class, AggregateClass::Distributive),
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_avg_sugar_lowers_to_algebraic_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "avg", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(210));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => assert_eq!(agg.class, AggregateClass::Algebraic),
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_count_sugar_lowers_to_distributive_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "count", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(211));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => assert_eq!(agg.class, AggregateClass::Distributive),
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Chains
// -----------------------------------------------------------------------------

#[test]
fn queryable_filter_map_sum_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let filter_param = param_pat(&mut tcx);
    let filter_body = bool_lit_expr(&mut tcx, true);
    let filter_closure = closure_expr(&mut tcx, filter_param, filter_body);
    let filtered = method_call_expr(&mut tcx, src, "filter", vec![filter_closure]);

    let map_param = param_pat(&mut tcx);
    let map_body = lit_expr(&mut tcx, 1);
    let map_closure = closure_expr(&mut tcx, map_param, map_body);
    let mapped = method_call_expr(&mut tcx, filtered, "map", vec![map_closure]);

    let sum = method_call_expr(&mut tcx, mapped, "sum", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, filtered, q, DefId::new(200));
    record_method_resolution(&mut results, mapped, q, DefId::new(201));
    record_method_resolution(&mut results, sum, q, DefId::new(209));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(filter_closure, dummy_ty());
    results.expr_types.insert(map_closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, sum, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    let agg_op = match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => agg,
        other => panic!("expected Aggregate, got {:?}", other),
    };
    assert_eq!(agg_op.class, AggregateClass::Distributive);

    let map_input = match plan.operator(lir) {
        LirOp::Aggregate { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(map_input), LirOp::Map { .. }));

    let filter_input = match plan.operator(map_input) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(filter_input), LirOp::Filter { .. }));
}

#[test]
fn queryable_take_after_filter_is_slice() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let pred_body = bool_lit_expr(&mut tcx, true);
    let closure = closure_expr(&mut tcx, param, pred_body);
    let filtered = method_call_expr(&mut tcx, src, "filter", vec![closure]);
    let take_n = lit_expr(&mut tcx, 5);
    let taken = method_call_expr(&mut tcx, filtered, "take", vec![take_n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, filtered, q, DefId::new(200));
    record_method_resolution(&mut results, taken, q, DefId::new(203));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, taken, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Slice { .. }));
}

// -----------------------------------------------------------------------------
// Binder mapping in closures
// -----------------------------------------------------------------------------

#[test]
fn closure_parameter_maps_to_binder() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let closure = closure_expr(&mut tcx, param, param_use);
    let call = method_call_expr(&mut tcx, src, "map", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(201));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    let projection = match plan.operator(lir) {
        LirOp::Map { projection, .. } => *projection,
        other => panic!("expected Map, got {:?}", other),
    };
    match plan.expr(projection) {
        QExpr::Closure { body, .. } => match plan.expr(*body) {
            QExpr::Column(_, _) => {}
            other => panic!("expected Column for closure parameter, got {:?}", other),
        },
        other => panic!("expected Closure projection, got {:?}", other),
    }
}

#[test]
fn closure_field_access_maps_to_binder_and_field() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let field_access = field_expr(&mut tcx, param_use, "age");
    let closure = closure_expr(&mut tcx, param, field_access);
    let call = method_call_expr(&mut tcx, src, "map", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(201));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());
    results.expr_types.insert(field_access, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    let projection = match plan.operator(lir) {
        LirOp::Map { projection, .. } => *projection,
        other => panic!("expected Map, got {:?}", other),
    };
    match plan.expr(projection) {
        QExpr::Closure { body, .. } => match plan.expr(*body) {
            QExpr::Field(base, _, _) => match plan.expr(*base) {
                QExpr::Column(_, _) => {}
                other => panic!("expected Column base, got {:?}", other),
            },
            other => panic!("expected Field body, got {:?}", other),
        },
        other => panic!("expected Closure projection, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Fallback / non-queryable method calls
// -----------------------------------------------------------------------------

#[test]
fn non_queryable_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "is_empty", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    // No method resolution: falls back to MethodCall expression.
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, .. } => assert_eq!(*method, DefId::new(1)),
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Select query lowering
// -----------------------------------------------------------------------------

#[test]
fn select_query_where_lowers_to_filter() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let where_clause = bool_lit_expr(&mut tcx, true);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: Some(where_clause),
            group_by: None,
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(where_clause, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // root should be Map(projection) over Filter(where)
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::Filter { .. }));
}

#[test]
fn select_query_order_by_lowers_to_order_by() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let key = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: key,
                direction: yelang_ast::query::SortDirection::Asc,
            }],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(key, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::OrderBy { .. }));
}

#[test]
fn select_query_range_lowers_to_slice() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: lit_expr(&mut tcx, 1),
                direction: yelang_ast::query::SortDirection::Asc,
            }],
            range: Some(yelang_hir::hir::query::QueryRange {
                start: Some(lit_expr(&mut tcx, 20)),
                end: Some(lit_expr(&mut tcx, 30)),
                inclusive: false,
            }),
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::Slice { .. }));
}

#[test]
fn select_query_group_by_lowers_to_group_by() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let key = lit_expr(&mut tcx, 1);
    let group_binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: Some(yelang_hir::hir::query::GroupByClause {
                keys: vec![yelang_hir::hir::query::GroupByKey {
                    name: None,
                    expr: key,
                }],
                into: ident("groups"),
                into_binder: group_binder,
            }),
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(key, dummy_ty());
    results.pat_types.insert(group_binder, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::GroupBy { .. }));
}


// -----------------------------------------------------------------------------
// Aggregate trait method fallback
// -----------------------------------------------------------------------------

#[test]
fn aggregate_classify_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let agg = aggregate_def(&tcx);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(300) },
        },
        Span::default(),
    );
    let call = method_call_expr(&mut tcx, marker, "classify", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, agg, DefId::new(400));
    results.expr_types.insert(marker, dummy_ty());

    let (plan, qexpr) = lower_single_expr_without_type(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, .. } => assert_eq!(*method, DefId::new(400)),
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

#[test]
fn aggregate_iterate_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let agg = aggregate_def(&tcx);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(300) },
        },
        Span::default(),
    );
    let acc = lit_expr(&mut tcx, 0);
    let value = lit_expr(&mut tcx, 1);
    let call = method_call_expr(&mut tcx, marker, "iterate", vec![acc, value]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, agg, DefId::new(401));
    results.expr_types.insert(marker, dummy_ty());

    let (plan, qexpr) = lower_single_expr_without_type(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, args, .. } => {
            assert_eq!(*method, DefId::new(401));
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Iterator / IntoIterator trait method fallback
// -----------------------------------------------------------------------------

#[test]
fn iterator_next_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let it = iterator_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "next", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, it, DefId::new(500));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr_without_type(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, .. } => assert_eq!(*method, DefId::new(500)),
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

#[test]
fn into_iter_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let ii = into_iter_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "into_iter", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, ii, DefId::new(501));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr_without_type(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, .. } => assert_eq!(*method, DefId::new(501)),
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

#[test]
fn iterator_map_method_call_falls_back_to_qexpr() {
    let mut tcx = mk_tcx();
    let it = iterator_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let body = lit_expr(&mut tcx, 2);
    let closure = closure_expr(&mut tcx, param, body);
    let call = method_call_expr(&mut tcx, src, "map", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, it, DefId::new(502));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr_without_type(&mut tcx, call, &mut results, &[], &[]);
    match plan.expr(qexpr) {
        QExpr::MethodCall { method, .. } => assert_eq!(*method, DefId::new(502)),
        other => panic!("expected MethodCall fallback, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Execute, aggregate markers, and operator chains
// -----------------------------------------------------------------------------

#[test]
fn queryable_execute_returns_input_subplan() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "execute", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(212));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Scan { .. }));
}

#[test]
fn queryable_aggregate_with_holistic_marker() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(301) },
        },
        Span::default(),
    );
    let call = method_call_expr(&mut tcx, src, "aggregate", vec![marker]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(208));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(marker, dummy_ty());

    let (plan, qexpr) = lower_single_expr(
        &mut tcx,
        call,
        &mut results,
        &[],
        &[(DefId::new(301), AggregateClass::Holistic)],
    );
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => assert_eq!(agg.class, AggregateClass::Holistic),
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_aggregate_with_algebraic_marker() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(302) },
        },
        Span::default(),
    );
    let call = method_call_expr(&mut tcx, src, "aggregate", vec![marker]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(208));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(marker, dummy_ty());

    let (plan, qexpr) = lower_single_expr(
        &mut tcx,
        call,
        &mut results,
        &[],
        &[(DefId::new(302), AggregateClass::Algebraic)],
    );
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, .. } => assert_eq!(agg.class, AggregateClass::Algebraic),
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_map_avg_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let param = param_pat(&mut tcx);
    let map_body = lit_expr(&mut tcx, 1);
    let closure = closure_expr(&mut tcx, param, map_body);
    let mapped = method_call_expr(&mut tcx, src, "map", vec![closure]);
    let avg = method_call_expr(&mut tcx, mapped, "avg", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, mapped, q, DefId::new(201));
    record_method_resolution(&mut results, avg, q, DefId::new(210));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, avg, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, input, .. } => {
            assert_eq!(agg.class, AggregateClass::Algebraic);
            assert!(matches!(plan.operator(*input), LirOp::Map { .. }));
        }
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_order_by_take_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let param = param_pat(&mut tcx);
    let key_body = lit_expr(&mut tcx, 1);
    let key_closure = closure_expr(&mut tcx, param, key_body);
    let ordered = method_call_expr(&mut tcx, src, "order_by", vec![key_closure]);
    let n = lit_expr(&mut tcx, 5);
    let taken = method_call_expr(&mut tcx, ordered, "take", vec![n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, ordered, q, DefId::new(205));
    record_method_resolution(&mut results, taken, q, DefId::new(203));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(key_closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, taken, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Slice { .. }));
    let slice_input = match plan.operator(lir) {
        LirOp::Slice { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(slice_input), LirOp::OrderBy { .. }));
}

#[test]
fn queryable_filter_skip_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let param = param_pat(&mut tcx);
    let pred_body = bool_lit_expr(&mut tcx, true);
    let closure = closure_expr(&mut tcx, param, pred_body);
    let filtered = method_call_expr(&mut tcx, src, "filter", vec![closure]);
    let n = lit_expr(&mut tcx, 3);
    let skipped = method_call_expr(&mut tcx, filtered, "skip", vec![n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, filtered, q, DefId::new(200));
    record_method_resolution(&mut results, skipped, q, DefId::new(204));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, skipped, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Slice { .. }));
    let slice_input = match plan.operator(lir) {
        LirOp::Slice { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(slice_input), LirOp::Filter { .. }));
}

#[test]
fn queryable_distinct_count_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let distinct = method_call_expr(&mut tcx, src, "distinct", vec![]);
    let count = method_call_expr(&mut tcx, distinct, "count", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, distinct, q, DefId::new(206));
    record_method_resolution(&mut results, count, q, DefId::new(211));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, count, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, input, .. } => {
            assert_eq!(agg.class, AggregateClass::Distributive);
            assert!(matches!(plan.operator(*input), LirOp::Distinct { .. }));
        }
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_flat_map_filter_sum_chain() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let fm_param = param_pat(&mut tcx);
    let fm_body = array_of_ints(&mut tcx, &[1]);
    let fm_closure = closure_expr(&mut tcx, fm_param, fm_body);
    let flat_mapped = method_call_expr(&mut tcx, src, "flat_map", vec![fm_closure]);

    let filter_param = param_pat(&mut tcx);
    let filter_body = bool_lit_expr(&mut tcx, true);
    let filter_closure = closure_expr(&mut tcx, filter_param, filter_body);
    let filtered = method_call_expr(&mut tcx, flat_mapped, "filter", vec![filter_closure]);

    let sum = method_call_expr(&mut tcx, filtered, "sum", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, flat_mapped, q, DefId::new(202));
    record_method_resolution(&mut results, filtered, q, DefId::new(200));
    record_method_resolution(&mut results, sum, q, DefId::new(209));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(fm_closure, dummy_ty());
    results.expr_types.insert(filter_closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, sum, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { input, .. } => {
            let filter_id = *input;
            assert!(matches!(plan.operator(filter_id), LirOp::Filter { .. }));
            let flat_map_id = match plan.operator(filter_id) {
                LirOp::Filter { input, .. } => *input,
                _ => unreachable!(),
            };
            assert!(matches!(plan.operator(flat_map_id), LirOp::FlatMap { .. }));
        }
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

// -----------------------------------------------------------------------------
// Select with combined clauses
// -----------------------------------------------------------------------------

#[test]
fn select_query_where_order_range() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let where_clause = bool_lit_expr(&mut tcx, true);
    let key = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: Some(where_clause),
            group_by: None,
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: key,
                direction: yelang_ast::query::SortDirection::Desc,
            }],
            range: Some(yelang_hir::hir::query::QueryRange {
                start: Some(lit_expr(&mut tcx, 10)),
                end: Some(lit_expr(&mut tcx, 20)),
                inclusive: false,
            }),
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(where_clause, dummy_ty());
    results.expr_types.insert(key, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // root: Map -> Slice -> OrderBy -> Filter -> Scan
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let mut cur = lir;
    cur = match plan.operator(cur) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::Slice { .. }));
    cur = match plan.operator(cur) {
        LirOp::Slice { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::OrderBy { .. }));
    cur = match plan.operator(cur) {
        LirOp::OrderBy { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::Filter { .. }));
}

#[test]
fn select_query_group_order() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let key = lit_expr(&mut tcx, 1);
    let group_binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: Some(yelang_hir::hir::query::GroupByClause {
                keys: vec![yelang_hir::hir::query::GroupByKey {
                    name: None,
                    expr: key,
                }],
                into: ident("groups"),
                into_binder: group_binder,
            }),
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: lit_expr(&mut tcx, 2),
                direction: yelang_ast::query::SortDirection::Asc,
            }],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(key, dummy_ty());
    results.pat_types.insert(group_binder, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // root: Map -> OrderBy -> GroupBy -> Scan
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let mut cur = lir;
    cur = match plan.operator(cur) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::OrderBy { .. }));
    cur = match plan.operator(cur) {
        LirOp::OrderBy { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::GroupBy { .. }));
}

#[test]
fn select_query_where_group() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let where_clause = bool_lit_expr(&mut tcx, true);
    let key = lit_expr(&mut tcx, 1);
    let group_binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: Some(where_clause),
            group_by: Some(yelang_hir::hir::query::GroupByClause {
                keys: vec![yelang_hir::hir::query::GroupByKey {
                    name: None,
                    expr: key,
                }],
                into: ident("groups"),
                into_binder: group_binder,
            }),
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(where_clause, dummy_ty());
    results.expr_types.insert(key, dummy_ty());
    results.pat_types.insert(group_binder, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // root: Map -> GroupBy -> Filter -> Scan
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let mut cur = lir;
    cur = match plan.operator(cur) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::GroupBy { .. }));
    cur = match plan.operator(cur) {
        LirOp::GroupBy { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(cur), LirOp::Filter { .. }));
}

// -----------------------------------------------------------------------------
// FROM node modifiers
// -----------------------------------------------------------------------------

#[test]
fn select_from_with_per_root_filter() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);
    let root_filter = bool_lit_expr(&mut tcx, true);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: Some(root_filter),
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());
    results.expr_types.insert(root_filter, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::Filter { .. }));
}

// -----------------------------------------------------------------------------
// Comprehension lowering
// -----------------------------------------------------------------------------

#[test]
fn comprehension_lowers_to_scan_map() {
    let mut tcx = mk_tcx();
    let src = array_of_ints(&mut tcx, &[1, 2]);
    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let comp = comprehension_expr(&mut tcx, param_use, param, src, None);

    let mut results = TypeckResults::new(DefId::new(1));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());
    results.expr_types.insert(comp, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, comp, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let map_input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(map_input), LirOp::Scan { .. }));
}

#[test]
fn comprehension_with_condition_lowers_to_filter_map() {
    let mut tcx = mk_tcx();
    let src = array_of_ints(&mut tcx, &[1, 2]);
    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let condition = bool_lit_expr(&mut tcx, true);
    let comp = comprehension_expr(&mut tcx, param_use, param, src, Some(condition));

    let mut results = TypeckResults::new(DefId::new(1));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());
    results.expr_types.insert(condition, dummy_ty());
    results.expr_types.insert(comp, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, comp, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let map_input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(map_input), LirOp::Filter { .. }));
}

#[test]
fn comprehension_binder_maps_to_column() {
    let mut tcx = mk_tcx();
    let src = array_of_ints(&mut tcx, &[1]);
    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let comp = comprehension_expr(&mut tcx, param_use, param, src, None);

    let mut results = TypeckResults::new(DefId::new(1));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());
    results.expr_types.insert(comp, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, comp, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    let projection = match plan.operator(lir) {
        LirOp::Map { projection, .. } => *projection,
        other => panic!("expected Map, got {:?}", other),
    };
    assert!(matches!(plan.expr(projection), QExpr::Column(_, _)));
}

// -----------------------------------------------------------------------------
// Error paths
// -----------------------------------------------------------------------------

#[test]
fn unregistered_queryable_method_returns_unsupported_expr() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "unknown", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(999));
    results.expr_types.insert(src, dummy_ty());

    let err = try_lower_single_expr(&mut tcx, call, &mut results, &[], &[])
        .expect_err("should fail");
    assert!(matches!(err, LoweringError::UnsupportedExpr));
}

#[test]
fn missing_aggregate_class_returns_missing_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let call = method_call_expr(&mut tcx, src, "sum", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    results.expr_types.insert(src, dummy_ty());
    record_method_resolution(&mut results, call, q, DefId::new(209));
    // Register sum method but DO NOT register its aggregate class.
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results)
        .with_queryable_method(DefId::new(209), QueryableMethod::Sum);

    let mut plan = LogicalPlan::empty();
    let err = yelang_qir::logical::lower_expr::lower_hir_expr(&mut plan, &mut ctx, call)
        .expect_err("should fail");
    assert!(matches!(err, LoweringError::MissingAggregate(_)));
}

#[test]
fn aggregate_marker_without_class_returns_missing_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let marker = tcx.crate_hir_mut().alloc_expr(
        Expr::Path {
            res: Res::Def { def_id: DefId::new(303) },
        },
        Span::default(),
    );
    let call = method_call_expr(&mut tcx, src, "aggregate", vec![marker]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(208));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(marker, dummy_ty());

    let err = try_lower_single_expr(&mut tcx, call, &mut results, &[], &[])
        .expect_err("should fail");
    assert!(matches!(err, LoweringError::MissingAggregate(_)));
}

#[test]
fn slice_on_unordered_returns_error() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![],
            range: Some(yelang_hir::hir::query::QueryRange {
                start: Some(lit_expr(&mut tcx, 5)),
                end: Some(lit_expr(&mut tcx, 10)),
                inclusive: false,
            }),
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let err = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect_err("should fail because source is unordered");
    assert!(matches!(err, LoweringError::SliceOnUnordered));
}

// -----------------------------------------------------------------------------
// Edge cases and robustness
// -----------------------------------------------------------------------------

#[test]
fn queryable_group_by_then_aggregate() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let key_param = param_pat(&mut tcx);
    let key_body = lit_expr(&mut tcx, 1);
    let key_closure = closure_expr(&mut tcx, key_param, key_body);
    let grouped = method_call_expr(&mut tcx, src, "group_by", vec![key_closure]);
    let count = method_call_expr(&mut tcx, grouped, "count", vec![]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, grouped, q, DefId::new(207));
    record_method_resolution(&mut results, count, q, DefId::new(211));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(key_closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, count, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Aggregate { agg, input, .. } => {
            assert_eq!(agg.class, AggregateClass::Distributive);
            assert!(matches!(plan.operator(*input), LirOp::GroupBy { .. }));
        }
        other => panic!("expected Aggregate, got {:?}", other),
    }
}

#[test]
fn queryable_take_zero_lowers_to_slice() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);
    let n = lit_expr(&mut tcx, 0);
    let call = method_call_expr(&mut tcx, src, "take", vec![n]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(203));
    results.expr_types.insert(src, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Slice { offset, limit, .. } => {
            assert!(limit.is_some());
            assert!(matches!(plan.expr(*offset), QExpr::Lit(yelang_qir::expr::QLit::Int(0), _)));
        }
        other => panic!("expected Slice, got {:?}", other),
    }
}

#[test]
fn queryable_skip_then_order_by() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let n = lit_expr(&mut tcx, 3);
    let skipped = method_call_expr(&mut tcx, src, "skip", vec![n]);

    let param = param_pat(&mut tcx);
    let key_body = lit_expr(&mut tcx, 1);
    let key_closure = closure_expr(&mut tcx, param, key_body);
    let ordered = method_call_expr(&mut tcx, skipped, "order_by", vec![key_closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, skipped, q, DefId::new(204));
    record_method_resolution(&mut results, ordered, q, DefId::new(205));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(key_closure, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, ordered, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    assert!(matches!(plan.operator(lir), LirOp::OrderBy { .. }));
    let order_input = match plan.operator(lir) {
        LirOp::OrderBy { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(order_input), LirOp::Slice { .. }));
}

#[test]
fn queryable_filter_with_closure_param_field() {
    let mut tcx = mk_tcx();
    let q = queryable_def(&tcx);
    let src = array_of_ints(&mut tcx, &[1]);

    let param = param_pat(&mut tcx);
    let param_use = path_local_expr(&mut tcx, param);
    let field_access = field_expr(&mut tcx, param_use, "active");
    let closure = closure_expr(&mut tcx, param, field_access);
    let call = method_call_expr(&mut tcx, src, "filter", vec![closure]);

    let mut results = TypeckResults::new(DefId::new(1));
    record_method_resolution(&mut results, call, q, DefId::new(200));
    results.expr_types.insert(src, dummy_ty());
    results.expr_types.insert(closure, dummy_ty());
    results.expr_types.insert(param_use, dummy_ty());
    results.expr_types.insert(field_access, dummy_ty());

    let (plan, qexpr) = lower_single_expr(&mut tcx, call, &mut results, &[], &[]);
    let lir = unwrap_subplan(&plan, qexpr);
    match plan.operator(lir) {
        LirOp::Filter { predicate, .. } => {
            match plan.expr(*predicate) {
                QExpr::Closure { body, .. } => match plan.expr(*body) {
                    QExpr::Field(base, _, _) => assert!(matches!(plan.expr(*base), QExpr::Column(_, _))),
                    other => panic!("expected Field body, got {:?}", other),
                },
                other => panic!("expected Closure predicate, got {:?}", other),
            }
        }
        other => panic!("expected Filter, got {:?}", other),
    }
}

#[test]
fn select_empty_where_clause_no_filter() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // Map directly over Scan, no Filter in between.
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    assert!(matches!(plan.operator(input), LirOp::Scan { .. }));
}

#[test]
fn select_range_start_only() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: lit_expr(&mut tcx, 1),
                direction: yelang_ast::query::SortDirection::Asc,
            }],
            range: Some(yelang_hir::hir::query::QueryRange {
                start: Some(lit_expr(&mut tcx, 5)),
                end: None,
                inclusive: false,
            }),
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    match plan.operator(input) {
        LirOp::Slice { offset, limit, .. } => {
            assert!(limit.is_none());
            assert!(matches!(plan.expr(*offset), QExpr::Lit(yelang_qir::expr::QLit::Int(5), _)));
        }
        other => panic!("expected Slice, got {:?}", other),
    }
}

#[test]
fn select_range_end_only() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source = array_of_ints(&mut tcx, &[1]);
    let binder = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from = yelang_hir::hir::query::FromNode {
        source,
        label: Symbol::from(1),
        binder,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![yelang_hir::hir::query::OrderByPart {
                expr: lit_expr(&mut tcx, 1),
                direction: yelang_ast::query::SortDirection::Asc,
            }],
            range: Some(yelang_hir::hir::query::QueryRange {
                start: None,
                end: Some(lit_expr(&mut tcx, 10)),
                inclusive: false,
            }),
        }),
    };

    results.expr_types.insert(source, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    match plan.operator(input) {
        LirOp::Slice { offset, limit, .. } => {
            assert!(limit.is_some());
            assert!(matches!(plan.expr(*offset), QExpr::Lit(yelang_qir::expr::QLit::Int(0), _)));
        }
        other => panic!("expected Slice, got {:?}", other),
    }
}

#[test]
fn multiple_from_nodes_use_first_root() {
    let mut tcx = mk_tcx();
    let mut results = TypeckResults::new(DefId::new(1));

    let source1 = array_of_ints(&mut tcx, &[1]);
    let binder1 = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let source2 = array_of_ints(&mut tcx, &[2]);
    let binder2 = tcx.crate_hir_mut().alloc_pat(Pat::Wild, Span::default());
    let projection = lit_expr(&mut tcx, 1);

    let from1 = yelang_hir::hir::query::FromNode {
        source: source1,
        label: Symbol::from(1),
        binder: binder1,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };
    let from2 = yelang_hir::hir::query::FromNode {
        source: source2,
        label: Symbol::from(2),
        binder: binder2,
        elem_ty: None,
        filter: None,
        order_by: vec![],
        range: None,
    };

    let query = yelang_hir::hir::query::Query {
        kind: yelang_hir::hir::query::QueryKind::Select(yelang_hir::hir::query::SelectQuery {
            projection,
            from: vec![from1, from2],
            links_match_kind: Default::default(),
            links: vec![],
            post_links_for: vec![],
            where_clause: None,
            group_by: None,
            order_by: vec![],
            range: None,
        }),
    };

    results.expr_types.insert(source1, dummy_ty());
    results.expr_types.insert(source2, dummy_ty());
    results.expr_types.insert(projection, dummy_ty());

    let mut plan = LogicalPlan::empty();
    let mut ctx = LoweringCtxt::new(&tcx, BodyId::default(), &results);
    let lir = yelang_qir::logical::lower_query::lower_query(&mut plan, &mut ctx, &query)
        .expect("lower query");
    plan.set_root(lir);

    // Projection is Map over the first source Scan.
    assert!(matches!(plan.operator(lir), LirOp::Map { .. }));
    let input = match plan.operator(lir) {
        LirOp::Map { input, .. } => *input,
        _ => unreachable!(),
    };
    match plan.operator(input) {
        LirOp::Scan { source, .. } => {
            assert!(matches!(source, yelang_qir::logical::operator::ScanSource::Expr(_)));
        }
        other => panic!("expected Scan, got {:?}", other),
    }
}
