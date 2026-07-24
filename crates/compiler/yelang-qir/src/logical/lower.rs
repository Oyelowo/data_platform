//! Plan extraction — builds a [`PlanArena`] from HIR query nodes.
//!
//! The main entry point is [`lower_query`], which dispatches on
//! [`QueryKind`] and builds the logical operator tree. Currently only
//! `Select` is implemented; mutations (`Create`, `Update`, …) will be
//! added later.
//!
//! # Evaluation order
//!
//! The plan tree mirrors the semantic evaluation order of a `select`:
//!
//! ```text
//! Scan (from)
//!   → Filter (from-node where)
//!     → Traverse (links)
//!       → Filter (pipeline where)
//!         → Aggregate (group by)
//!           → Sort (order by)
//!             → Limit (range)
//!               → Project / Map (projection)
//! ```

pub mod method_kind;
pub mod resolve;

use self::method_kind::QueryableMethod;
use yelang_ast::query::{EdgeDirection, SortDirection};
use yelang_hir::hir::expr::{ComprehensionKind, ComprehensionVar, Expr};
use yelang_hir::hir::query::{
    FromNode, GroupByClause, OrderByPart, QueryKind, SelectLinkPath,
    SelectLinkSegment, SelectQuery,
};
use yelang_hir::ids::{ExprId, PatId, QueryId};
use yelang_hir::Crate;
use yelang_interner::{Interner, Symbol};
use yelang_resolve::lang_items::{LangItem, LangItems};
use yelang_thir::ids::{ThirExprId, ThirPatId};
use yelang_thir::query::{
    ThirDirection, ThirFromNode, ThirGroupBy, ThirLinkPath, ThirLinkSegment, ThirOrderByPart,
    ThirSelectQuery,
};
use yelang_thir::{ThirBodies, ThirExpr, ThirPat};

use crate::logical::plan::{
    AggCall, AggKind, Direction, EdgeRef, GroupKey, JoinKind, NodeRef, Plan, PlanArena,
    PlanId, PlanOrigin, PlanRange, SortKey, SortSpec, SourceRef, TraversePath, TraverseSegment,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract a logical plan tree from a query.
///
/// Returns the root [`PlanId`] of the extracted tree, allocated in `arena`.
///
/// When `thir_bodies` is `Some` and contains a lowered [`ThirSelectQuery`] for
/// `query_id`, the plan is built directly from THIR (no HIR query dependency).
/// Otherwise it falls back to reading the HIR query node.
pub fn lower_query(
    query_id: QueryId,
    hir: &Crate,
    thir_bodies: Option<&ThirBodies>,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    // Prefer the THIR query structure when available.
    if let Some(bodies) = thir_bodies {
        if let Some(select) = bodies.thir_queries.get(&query_id) {
            return Some(lower_select(select, PlanOrigin::QuerySyntax(query_id), bodies, interner, arena));
        }
    }

    // Fall back to the HIR query node.
    let query = hir.query(query_id)?;
    match &query.kind {
        QueryKind::Select(select) => {
            Some(lower_select_hir(select, query_id, hir, interner, lang_items, arena))
        }
        QueryKind::Create(_)
        | QueryKind::Update(_)
        | QueryKind::Upsert(_)
        | QueryKind::Delete(_)
        | QueryKind::Link(_)
        | QueryKind::Unlink(_) => None,
    }
}

// ---------------------------------------------------------------------------
// Select extraction (THIR-driven)
// ---------------------------------------------------------------------------

/// Build a logical plan tree directly from a [`ThirSelectQuery`].
///
/// All sub-expressions are already THIR expression IDs ([`ExprRef`]), so no
/// HIR→THIR conversion is needed. Aggregate decomposition over the projection
/// is a HIR-based post-process and is not performed here (see the HIR fallback
/// path [`lower_select_hir`]).
fn lower_select(
    select: &ThirSelectQuery,
    origin: PlanOrigin,
    thir_bodies: &ThirBodies,
    interner: &Interner,
    arena: &mut PlanArena,
) -> PlanId {

    // 1. Build scan(s) from `from` nodes.
    let mut current = lower_from_nodes_thir(&select.from, thir_bodies, arena, &origin);

    // 2. Apply `links` traversals.
    if !select.links.is_empty() {
        let paths = lower_link_paths_thir(&select.links, thir_bodies);
        current = alloc(
            arena,
            Plan::Traverse {
                input: current,
                paths,
            },
            origin.clone(),
        );
    }

    // 3. Apply pipeline `where` (post-links filter).
    // Also extract correlated subqueries from the WHERE predicate.
    if let Some(pred) = select.where_clause {
        let (new_current, new_pred) = extract_correlated_subqueries(
            current,
            pred,
            thir_bodies,
            interner,
            arena,
            &origin,
        );
        current = new_current;
        current = alloc(
            arena,
            Plan::Filter { input: current, pred: new_pred },
            origin.clone(),
        );
    }

    // 4. Apply `group by`.
    if let Some(group_by) = &select.group_by {
        current = lower_group_by_thir(current, group_by, arena, &origin);
    }

    // 5. Apply `order by`.
    if !select.order_by.is_empty() {
        let specs = lower_order_specs_thir(&select.order_by);
        current = alloc(
            arena,
            Plan::Sort {
                input: current,
                specs,
            },
            origin.clone(),
        );
    }

    // 6. Apply `range`.
    if let Some(range) = &select.range {
        current = alloc(
            arena,
            Plan::Limit {
                input: current,
                skip: range.start,
                fetch: range.end,
            },
            origin.clone(),
        );
    }

    // 7. Extract correlated subqueries from the projection.
    //
    // Walk the projection expression for ThirExpr::Query nodes.
    // For each correlated subquery, create a DependentJoin and replace
    // the Query node with a column reference to the join's output.
    let (mut current, projection) = extract_correlated_subqueries(
        current,
        select.projection,
        thir_bodies,
        interner,
        arena,
        &origin,
    );

    // 8. Apply projection.
    let result_name = interner.intern("result");
    current = alloc(
        arena,
        Plan::Project {
            input: current,
            exprs: vec![(result_name, projection)],
        },
        origin,
    );

    current
}

// ---------------------------------------------------------------------------
// THIR from nodes → Scan (+ optional Filter, Sort, Limit)
// ---------------------------------------------------------------------------

fn lower_from_nodes_thir(
    from: &[ThirFromNode],
    thir_bodies: &ThirBodies,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    debug_assert!(!from.is_empty(), "select must have at least one from node");

    let mut scans: Vec<PlanId> = Vec::with_capacity(from.len());

    for node in from {
        let source = resolve_source_ref_thir(node.source, node.label, thir_bodies);
        let mut scan_id = alloc(
            arena,
            Plan::Scan {
                source,
                filter: None,
                projection: None,
                range: None,
            },
            origin.clone(),
        );

        // Per-root modifiers.
        if let Some(filter) = node.filter {
            scan_id = alloc(
                arena,
                Plan::Filter {
                    input: scan_id,
                    pred: filter,
                },
                origin.clone(),
            );
        }
        if !node.order_by.is_empty() {
            let specs = lower_order_specs_thir(&node.order_by);
            scan_id = alloc(
                arena,
                Plan::Sort {
                    input: scan_id,
                    specs,
                },
                origin.clone(),
            );
        }
        if let Some(range) = &node.range {
            scan_id = alloc(
                arena,
                Plan::Limit {
                    input: scan_id,
                    skip: range.start,
                    fetch: range.end,
                },
                origin.clone(),
            );
        }

        scans.push(scan_id);
    }

    if scans.len() == 1 {
        return scans[0];
    }

    // Multiple roots: cross-join left-to-right.
    let mut result = scans[0];
    for &right in &scans[1..] {
        result = alloc(
            arena,
            Plan::Join {
                left: result,
                right,
                kind: JoinKind::Cross,
                on: vec![],
                filter: None,
            },
            origin.clone(),
        );
    }
    result
}

/// Determine the [`SourceRef`] for a THIR `from` node's source expression.
///
/// - `ThirExpr::Var` → table-backed source (the DefId points to a struct)
/// - `ThirExpr::Local` → local variable holding a collection
/// - Other expressions → function/method call returning a collection
fn resolve_source_ref_thir(
    source: ThirExprId,
    label: Symbol,
    thir_bodies: &ThirBodies,
) -> SourceRef {
    match thir_bodies.exprs.get(source) {
        Some(ThirExpr::Var(def_id)) => SourceRef::Table {
            def: *def_id,
            name: label,
        },
        Some(ThirExpr::Local(_)) => SourceRef::Local { name: label },
        // Non-path expressions (calls, etc.) → Call source.
        _ => SourceRef::Call { func: source },
    }
}

// ---------------------------------------------------------------------------
// THIR links → TraversePath
// ---------------------------------------------------------------------------

fn lower_link_paths_thir(links: &[ThirLinkPath], thir_bodies: &ThirBodies) -> Vec<TraversePath> {
    links
        .iter()
        .map(|path| lower_link_path_thir(path, thir_bodies))
        .collect()
}

fn lower_link_path_thir(path: &ThirLinkPath, thir_bodies: &ThirBodies) -> TraversePath {
    let anchor = path.anchor.label;

    let segments = path
        .segments
        .iter()
        .map(|seg| lower_link_segment_thir(seg, thir_bodies))
        .collect();

    TraversePath { anchor, segments }
}

fn lower_link_segment_thir(seg: &ThirLinkSegment, thir_bodies: &ThirBodies) -> TraverseSegment {
    let direction = match seg.direction {
        ThirDirection::Forward => Direction::Forward,
        ThirDirection::Backward => Direction::Backward,
        ThirDirection::Both => Direction::Both,
    };

    let edge = EdgeRef {
        // TODO: resolve the actual DefId from the edge type annotation.
        def: yelang_arena::DefId::new(1),
        label: seg.edge.label,
        binder: binder_symbol_thir(seg.edge.binder, thir_bodies),
    };

    let target = NodeRef {
        // TODO: resolve the actual DefId from the target type annotation.
        def: yelang_arena::DefId::new(1),
        label: seg.target.label,
        binder: binder_symbol_thir(seg.target.binder, thir_bodies),
    };

    TraverseSegment {
        edge,
        direction,
        target,
        edge_pred: seg.edge.filter,
        target_pred: seg.target.filter,
        hop_range: seg.hop_range.as_ref().map(|h| PlanRange {
            start: h.start,
            end: h.end,
            inclusive: h.inclusive,
        }),
    }
}

// ---------------------------------------------------------------------------
// THIR group by → Aggregate
// ---------------------------------------------------------------------------

fn lower_group_by_thir(
    input: PlanId,
    group_by: &ThirGroupBy,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    let keys: Vec<(Symbol, GroupKey)> = group_by
        .keys
        .iter()
        .map(|(name, expr)| (*name, GroupKey::Expr(*expr)))
        .collect();

    alloc(
        arena,
        Plan::Aggregate {
            input,
            keys,
            // Aggregate calls are populated by the HIR-based projection
            // decomposition pass (not run on the THIR path).
            aggs: vec![],
            into: group_by.into,
        },
        origin.clone(),
    )
}

// ---------------------------------------------------------------------------
// THIR order by → SortSpec
// ---------------------------------------------------------------------------

fn lower_order_specs_thir(parts: &[ThirOrderByPart]) -> Vec<SortSpec> {
    parts
        .iter()
        .map(|part| SortSpec {
            key: SortKey::Expr(part.expr),
            desc: part.desc,
        })
        .collect()
}

/// Extract the binder symbol from a THIR [`ThirPatId`].
fn binder_symbol_thir(pat_id: ThirPatId, thir_bodies: &ThirBodies) -> Symbol {
    match thir_bodies.pat(pat_id) {
        Some(ThirPat::Binding { name, .. }) => *name,
        // Fallback for non-binding patterns (wildcard, etc.)
        _ => Symbol::from(1u32),
    }
}

// ---------------------------------------------------------------------------
// Select extraction (HIR fallback)
// ---------------------------------------------------------------------------

fn lower_select_hir(
    select: &SelectQuery,
    query_id: QueryId,
    hir: &Crate,
    interner: &Interner,
    _lang_items: &LangItems,
    arena: &mut PlanArena,
) -> PlanId {
    let origin = PlanOrigin::QuerySyntax(query_id);

    // 1. Build scan(s) from `from` nodes.
    let mut current = lower_from_nodes(&select.from, hir, arena, &origin);

    // 2. Apply `links` traversals.
    if !select.links.is_empty() {
        let paths = lower_link_paths(&select.links, hir, interner, arena);
        current = alloc(
            arena,
            Plan::Traverse {
                input: current,
                paths,
            },
            origin.clone(),
        );
    }

    // 3. Apply pipeline `where` (post-links filter).
    if let Some(pred) = select.where_clause {
        let thir_pred = arena.to_thir(pred);
        current = alloc(
            arena,
            Plan::Filter { input: current, pred: thir_pred },
            origin.clone(),
        );
    }

    // 4. Apply `group by`.
    if let Some(group_by) = &select.group_by {
        current = lower_group_by(current, group_by, interner, arena, &origin);
    }

    // 5. Apply `order by`.
    if !select.order_by.is_empty() {
        let specs = lower_order_specs(&select.order_by, arena);
        current = alloc(
            arena,
            Plan::Sort {
                input: current,
                specs,
            },
            origin.clone(),
        );
    }

    // 6. Apply `range`.
    if let Some(range) = &select.range {
        let skip = range.start.map(|e| arena.to_thir(e));
        let fetch = range.end.map(|e| arena.to_thir(e));
        current = alloc(
            arena,
            Plan::Limit {
                input: current,
                skip,
                fetch,
            },
            origin.clone(),
        );
    }

    // 7. Apply projection.
    let result_name = interner.intern("result");
    let thir_projection = arena.to_thir(select.projection);
    current = alloc(
        arena,
        Plan::Project {
            input: current,
            exprs: vec![(result_name, thir_projection)],
        },
        origin,
    );

    // 8. Post-process: decompose the projection to populate Aggregate.aggs.
    decompose_projection_aggregates(current, select.projection, hir, interner, arena);

    current
}

// ---------------------------------------------------------------------------
// From nodes → Scan (+ optional Filter, Sort, Limit)
// ---------------------------------------------------------------------------

fn lower_from_nodes(
    from: &[FromNode],
    hir: &Crate,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    debug_assert!(!from.is_empty(), "select must have at least one from node");

    let mut scans: Vec<PlanId> = Vec::with_capacity(from.len());

    for node in from {
        let mut scan_id = lower_single_from(node, hir, arena, origin);

        // Per-root modifiers.
        if let Some(filter) = node.filter {
            let thir_filter = arena.to_thir(filter);
            scan_id = alloc(
                arena,
                Plan::Filter {
                    input: scan_id,
                    pred: thir_filter,
                },
                origin.clone(),
            );
        }
        if !node.order_by.is_empty() {
            let specs = lower_order_specs(&node.order_by, arena);
            scan_id = alloc(
                arena,
                Plan::Sort {
                    input: scan_id,
                    specs,
                },
                origin.clone(),
            );
        }
        if let Some(range) = &node.range {
            let skip = range.start.map(|e| arena.to_thir(e));
            let fetch = range.end.map(|e| arena.to_thir(e));
            scan_id = alloc(
                arena,
                Plan::Limit {
                    input: scan_id,
                    skip,
                    fetch,
                },
                origin.clone(),
            );
        }

        scans.push(scan_id);
    }

    if scans.len() == 1 {
        return scans[0];
    }

    // Multiple roots: cross-join left-to-right.
    let mut result = scans[0];
    for &right in &scans[1..] {
        result = alloc(
            arena,
            Plan::Join {
                left: result,
                right,
                kind: JoinKind::Cross,
                on: vec![],
                filter: None,
            },
            origin.clone(),
        );
    }
    result
}

fn lower_single_from(
    node: &FromNode,
    hir: &Crate,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    let source = resolve_source_ref(node.source, node.label, hir, arena);

    alloc(
        arena,
        Plan::Scan {
            source,
            filter: None,
            projection: None,
            range: None,
        },
        origin.clone(),
    )
}

/// Determine the [`SourceRef`] for a `from` node's source expression.
///
/// - `Res::Def` → table-backed source (the DefId points to a struct)
/// - `Res::Local` → local variable holding a collection
/// - Other expressions → function/method call returning a collection
fn resolve_source_ref(source_expr: ExprId, label: Symbol, hir: &Crate, arena: &PlanArena) -> SourceRef {
    match hir.expr(source_expr) {
        Some(Expr::Path { res }) => match res {
            yelang_hir::res::Res::Def { def_id } => SourceRef::Table {
                def: *def_id,
                name: label,
            },
            yelang_hir::res::Res::Local { .. } => SourceRef::Local { name: label },
            _ => SourceRef::Local { name: label },
        },
        // Non-path expressions (calls, etc.) → Call source.
        _ => SourceRef::Call { func: arena.to_thir(source_expr) },
    }
}

// ---------------------------------------------------------------------------
// Links → TraversePath
// ---------------------------------------------------------------------------

fn lower_link_paths(
    links: &[SelectLinkPath],
    hir: &Crate,
    interner: &Interner,
    arena: &PlanArena,
) -> Vec<TraversePath> {
    links
        .iter()
        .map(|path| lower_link_path(path, hir, interner, arena))
        .collect()
}

fn lower_link_path(
    path: &SelectLinkPath,
    hir: &Crate,
    interner: &Interner,
    arena: &PlanArena,
) -> TraversePath {
    let anchor = path.start.var.symbol;

    let segments = path
        .segments
        .iter()
        .map(|seg| lower_link_segment(seg, hir, interner, arena))
        .collect();

    TraversePath { anchor, segments }
}

fn lower_link_segment(
    seg: &SelectLinkSegment,
    hir: &Crate,
    _interner: &Interner,
    arena: &PlanArena,
) -> TraverseSegment {
    let direction = match seg.direction {
        EdgeDirection::Forward => Direction::Forward,
        EdgeDirection::Backward => Direction::Backward,
        EdgeDirection::Bidirectional => Direction::Both,
    };

    let edge = EdgeRef {
        // TODO: resolve the actual DefId from the edge type annotation.
        def: yelang_arena::DefId::new(1),
        label: seg.edge.var.symbol,
        binder: binder_symbol(seg.edge.binder, hir),
    };

    let target = NodeRef {
        // TODO: resolve the actual DefId from the target type annotation.
        def: yelang_arena::DefId::new(1),
        label: seg.target.var.symbol,
        binder: binder_symbol(seg.target.binder, hir),
    };

    TraverseSegment {
        edge,
        direction,
        target,
        edge_pred: seg.edge.modifiers.filter.map(|e| arena.to_thir(e)),
        target_pred: seg.target.modifiers.filter.map(|e| arena.to_thir(e)),
        hop_range: seg.edge.hops.as_ref().map(|h| PlanRange {
            start: h.start.map(|e| arena.to_thir(e)),
            end: h.end.map(|e| arena.to_thir(e)),
            inclusive: h.inclusive,
        }),
    }
}

// ---------------------------------------------------------------------------
// Group by → Aggregate
// ---------------------------------------------------------------------------

fn lower_group_by(
    input: PlanId,
    group_by: &GroupByClause,
    interner: &Interner,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    let fallback = interner.intern("_key");
    let keys: Vec<(Symbol, GroupKey)> = group_by
        .keys
        .iter()
        .map(|key| {
            let name = key.name.map(|ident| ident.symbol).unwrap_or(fallback);
            (name, GroupKey::Expr(arena.to_thir(key.expr)))
        })
        .collect();

    alloc(
        arena,
        Plan::Aggregate {
            input,
            keys,
            // Aggregate calls are populated by a later projection-decomposition
            // pass that recognizes sum/count/avg/etc. in the projection expr.
            aggs: vec![],
            into: group_by.into.symbol,
        },
        origin.clone(),
    )
}

// ---------------------------------------------------------------------------
// Projection decomposition — extract aggregates from the select expression
// ---------------------------------------------------------------------------

/// Walk the projection expression to find aggregate calls and populate
/// the `Aggregate` node's `aggs` list.
///
/// This is a post-processing step after the plan tree is built. It finds
/// the `Aggregate` node in the tree, then walks the projection expression
/// looking for known aggregate method calls (`sum`, `count`, `avg`, etc.)
/// and user-defined `Aggregate` trait calls.
fn decompose_projection_aggregates(
    root: PlanId,
    projection: ExprId,
    hir: &Crate,
    interner: &Interner,
    arena: &mut PlanArena,
) {
    // Find the Aggregate node in the plan tree.
    let agg_id = find_aggregate_node(root, arena);
    let Some(agg_id) = agg_id else {
        return; // No group by → no aggregates to decompose.
    };

    // Walk the projection expression to find aggregate calls.
    let mut aggs = Vec::new();
    collect_aggregate_calls(projection, hir, interner, arena, &mut aggs);

    if aggs.is_empty() {
        return;
    }

    // Update the Aggregate node's aggs list.
    if let Some(Plan::Aggregate { aggs: existing, .. }) = arena.get_mut(agg_id) {
        *existing = aggs;
    }
}

/// Find the first `Aggregate` node in the plan tree (walking down the spine).
fn find_aggregate_node(root: PlanId, arena: &PlanArena) -> Option<PlanId> {
    let mut current = Some(root);
    while let Some(id) = current {
        let plan = arena.plan(id);
        if matches!(plan, Plan::Aggregate { .. }) {
            return Some(id);
        }
        current = crate::tree::children(plan).into_iter().next();
    }
    None
}

/// Walk an expression tree and collect aggregate calls.
fn collect_aggregate_calls(
    expr: ExprId,
    hir: &Crate,
    interner: &Interner,
    arena: &PlanArena,
    out: &mut Vec<AggCall>,
) {
    let Some(expr_node) = hir.expr(expr) else {
        return;
    };

    match expr_node {
        // Method call: check if it's a known aggregate.
        Expr::MethodCall {
            receiver: _,
            method,
            args,
            ..
        } => {
            let name = method.symbol.as_str(interner);
            let output = interner.intern(&format!("_{}", name));

            let kind = match QueryableMethod::from_name(name) {
                Some(QueryableMethod::Sum) => Some(AggKind::Sum { expr: arena.to_thir(expr) }),
                Some(QueryableMethod::Count) => Some(AggKind::Count),
                Some(QueryableMethod::Avg) => Some(AggKind::Avg { expr: arena.to_thir(expr) }),
                Some(QueryableMethod::Min) => Some(AggKind::Min { expr: arena.to_thir(expr) }),
                Some(QueryableMethod::Max) => Some(AggKind::Max { expr: arena.to_thir(expr) }),
                Some(QueryableMethod::Aggregate) => {
                    // User-defined aggregate via .aggregate(Marker).
                    let marker = args.first().copied();
                    Some(AggKind::UserAggregate {
                        impl_def: yelang_arena::DefId::new(1), // TODO: resolve from trait impl
                        args: marker.map(|m| vec![arena.to_thir(m)]).unwrap_or_default(),
                        input_expr: None,
                        // TODO: resolve from the trait impl's properties() method
                        // via constant evaluation. For now, use conservative defaults.
                        properties: crate::logical::plan::AggProperties {
                            class: crate::logical::plan::AggClass::Holistic,
                            associative: false,
                            commutative: false,
                            invertible: false,
                        },
                    })
                }
                _ => None,
            };

            if let Some(kind) = kind {
                out.push(AggCall { output, kind });
            }

            // Recurse into arguments (there may be nested aggregates).
            for &arg in args {
                collect_aggregate_calls(arg, hir, interner, arena, out);
            }
        }

        // Recurse into sub-expressions.
        Expr::Binary { left, right, .. } => {
            collect_aggregate_calls(*left, hir, interner, arena, out);
            collect_aggregate_calls(*right, hir, interner, arena, out);
        }
        Expr::Unary { expr: inner, .. } => {
            collect_aggregate_calls(*inner, hir, interner, arena, out);
        }
        Expr::Call { func, args } => {
            collect_aggregate_calls(*func, hir, interner, arena, out);
            for &arg in args {
                collect_aggregate_calls(arg, hir, interner, arena, out);
            }
        }
        Expr::Field { expr: base, .. } => {
            collect_aggregate_calls(*base, hir, interner, arena, out);
        }
        Expr::Index { expr: base, index } => {
            collect_aggregate_calls(*base, hir, interner, arena, out);
            collect_aggregate_calls(*index, hir, interner, arena, out);
        }
        Expr::Cast { expr: inner, .. } | Expr::TypeAscription { expr: inner, .. } => {
            collect_aggregate_calls(*inner, hir, interner, arena, out);
        }
        Expr::Struct { fields, rest, .. } => {
            for field in fields {
                collect_aggregate_calls(field.expr, hir, interner, arena, out);
            }
            if let Some(rest_expr) = rest {
                collect_aggregate_calls(*rest_expr, hir, interner, arena, out);
            }
        }
        Expr::Object { fields } => {
            for field in fields {
                collect_aggregate_calls(field.expr, hir, interner, arena, out);
            }
        }
        Expr::Tuple { exprs } | Expr::Array { exprs } => {
            for &e in exprs {
                collect_aggregate_calls(e, hir, interner, arena, out);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_aggregate_calls(*cond, hir, interner, arena, out);
            collect_aggregate_calls(*then_branch, hir, interner, arena, out);
            if let Some(else_expr) = else_branch {
                collect_aggregate_calls(*else_expr, hir, interner, arena, out);
            }
        }
        Expr::Block { block } => {
            if let Some(tail) = block.expr {
                collect_aggregate_calls(tail, hir, interner, arena, out);
            }
        }
        Expr::Comprehension {
            element,
            variables,
            condition,
            ..
        } => {
            collect_aggregate_calls(*element, hir, interner, arena, out);
            for var in variables {
                collect_aggregate_calls(var.source, hir, interner, arena, out);
            }
            if let Some(cond) = condition {
                collect_aggregate_calls(*cond, hir, interner, arena, out);
            }
        }

        // Leaves: no aggregates.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Order by → OrderSpec
// ---------------------------------------------------------------------------

fn lower_order_specs(parts: &[OrderByPart], arena: &PlanArena) -> Vec<SortSpec> {
    parts
        .iter()
        .map(|part| SortSpec {
            key: SortKey::Expr(arena.to_thir(part.expr)),
            desc: matches!(part.direction, SortDirection::Desc),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn alloc(arena: &mut PlanArena, plan: Plan, origin: PlanOrigin) -> PlanId {
    arena.alloc_with_origin(plan, origin)
}

/// Extract the binder symbol from a `PatId`.
///
/// The binder pattern is typically a simple identifier pattern like `u`
/// in `users@u:User`.
fn binder_symbol(pat_id: PatId, hir: &Crate) -> Symbol {
    match hir.pat(pat_id) {
        Some(yelang_hir::hir::pat::Pat::Binding { name, .. }) => *name,
        // Fallback for non-binding patterns (wildcard, etc.)
        _ => Symbol::from(1u32),
    }
}

// ===========================================================================
// Method chain / expression extraction
// ===========================================================================

/// Try to interpret an HIR expression as a collection-producing plan.
///
/// This is the second entry point into the plan tree (alongside
/// [`lower_query`]). It handles:
/// - `Queryable` method chains: `.filter()`, `.map()`, `.join()`, …
/// - Comprehensions (desugared selectors): `[*]`, `[where …]`, `[**]`
/// - `@intrinsic(query_*)` calls
/// - Table/path references → `Scan`
/// - Nested `select` queries → recursive [`lower_query`]
///
/// Returns `None` if the expression cannot be interpreted as a collection
/// plan (e.g. a scalar literal, a non-Queryable function call).
pub fn lower_expr_as_plan(
    expr_id: ExprId,
    hir: &Crate,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let expr = hir.expr(expr_id)?;
    let origin = PlanOrigin::MethodCall(arena.to_thir(expr_id));

    match expr {
        // ── Queryable method call ──────────────────────────────────────
        Expr::MethodCall {
            receiver,
            method,
            args,
            trait_def_id,
        } => {
            // Verify this is a Queryable trait method via lang items.
            let is_queryable = trait_def_id
                .and_then(|tid| lang_items.get(LangItem::Queryable).map(|qid| tid == qid))
                .unwrap_or(false);

            if is_queryable {
                lower_method_call(expr_id, *receiver, method.symbol, args, hir, interner, lang_items, arena)
            } else {
                // Non-Queryable method: treat as opaque extension.
                let input = lower_expr_as_plan(*receiver, hir, interner, lang_items, arena);
                input.map(|inp| {
                    alloc(
                        arena,
                        Plan::Extension {
                            node: std::sync::Arc::new(OpaqueMethod {
                                name: method.symbol.as_str(interner).to_string(),
                                input: inp,
                                call: expr_id,
                            }),
                        },
                        origin,
                    )
                })
            }
        }

        // ── Comprehension (desugared selector) ─────────────────────────
        Expr::Comprehension {
            kind: ComprehensionKind::List,
            element,
            variables,
            condition,
        } => lower_comprehension(*element, variables, *condition, hir, interner, lang_items, arena),

        // ── Intrinsic call ─────────────────────────────────────────────
        Expr::Intrinsic { name, args } => {
            lower_intrinsic(name.symbol, args, hir, interner, lang_items, arena)
        }

        // ── Nested select query ────────────────────────────────────────
        Expr::Query(query_id) => lower_query(*query_id, hir, None, interner, lang_items, arena),

        // ── Path reference (table or local variable) ───────────────────
        Expr::Path { res } => {
            let source = match res {
                yelang_hir::res::Res::Def { def_id } => SourceRef::Table {
                    def: *def_id,
                    name: Symbol::from(1u32),
                },
                yelang_hir::res::Res::Local { .. } => SourceRef::Local {
                    name: Symbol::from(1u32),
                },
                _ => SourceRef::Local {
                    name: Symbol::from(1u32),
                },
            };
            Some(alloc(
                arena,
                Plan::Scan {
                    source,
                    filter: None,
                    projection: None,
                    range: None,
                },
                origin,
            ))
        }

        // ── Anything else: not a collection plan ───────────────────────
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Method call → Plan node
// ---------------------------------------------------------------------------

fn lower_method_call(
    call_expr: ExprId,
    receiver: ExprId,
    method: Symbol,
    args: &[ExprId],
    hir: &Crate,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let origin = PlanOrigin::MethodCall(arena.to_thir(call_expr));
    let method_name = method.as_str(interner);

    // Extract the receiver as a plan (the collection being operated on).
    let input = lower_expr_as_plan(receiver, hir, interner, lang_items, arena)?;

    // Dispatch via centralized QueryableMethod enum.
    let Some(mk) = method_kind::QueryableMethod::from_name(method_name) else {
        // Unknown method: opaque extension barrier.
        return Some(alloc(
            arena,
            Plan::Extension {
                node: std::sync::Arc::new(OpaqueMethod {
                    name: method_name.to_string(),
                    input,
                    call: call_expr,
                }),
            },
            origin,
        ));
    };

    match mk {
        // ── Filter ─────────────────────────────────────────────────────
        QueryableMethod::Filter => {
            // .filter(|x| pred) — args[0] is the closure
            let pred = args.first().copied()?;
            let thir_pred = arena.to_thir(pred);
            Some(alloc(
                arena,
                Plan::Filter { input, pred: thir_pred },
                origin,
            ))
        }

        // ── Map ────────────────────────────────────────────────────────
        QueryableMethod::Map => {
            // .map(|x| expr) — args[0] is the closure
            let func = args.first().copied()?;
            let thir_func = arena.to_thir(func);
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func: thir_func,
                    flatten_depth: 0,
                },
                origin,
            ))
        }

        // ── FlatMap ────────────────────────────────────────────────────
        QueryableMethod::FlatMap => {
            let func = args.first().copied()?;
            let thir_func = arena.to_thir(func);
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func: thir_func,
                    flatten_depth: 1,
                },
                origin,
            ))
        }

        // ── Joins ──────────────────────────────────────────────────────
        QueryableMethod::Join | QueryableMethod::InnerJoin => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = lower_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
            let thir_on = arena.to_thir(on_expr);
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Inner,
                    on: vec![], // TODO: decompose the closure into equi-join keys
                    filter: Some(thir_on),
                },
                origin,
            ))
        }

        QueryableMethod::LeftJoin => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = lower_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
            let thir_on = arena.to_thir(on_expr);
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Left,
                    on: vec![],
                    filter: Some(thir_on),
                },
                origin,
            ))
        }

        QueryableMethod::SemiJoin => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = lower_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
            let thir_on = arena.to_thir(on_expr);
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Semi,
                    on: vec![],
                    filter: Some(thir_on),
                },
                origin,
            ))
        }

        QueryableMethod::AntiJoin => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = lower_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
            let thir_on = arena.to_thir(on_expr);
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Anti,
                    on: vec![],
                    filter: Some(thir_on),
                },
                origin,
            ))
        }

        // ── Group by ───────────────────────────────────────────────────
        QueryableMethod::GroupBy => {
            // .group_by(|x| key_expr) — args[0] is the key closure
            let key_expr = args.first().copied()?;
            let thir_key = arena.to_thir(key_expr);
            let key_name = interner.intern("_key");
            let into = interner.intern("_groups");
            Some(alloc(
                arena,
                Plan::Aggregate {
                    input,
                    keys: vec![(key_name, GroupKey::Expr(thir_key))],
                    aggs: vec![],
                    into,
                },
                origin,
            ))
        }

        // ── Order by ───────────────────────────────────────────────────
        QueryableMethod::OrderBy | QueryableMethod::SortBy => {
            let key_expr = args.first().copied()?;
            let thir_key = arena.to_thir(key_expr);
            Some(alloc(
                arena,
                Plan::Sort {
                    input,
                    specs: vec![SortSpec {
                        key: SortKey::Expr(thir_key),
                        desc: false,
                    }],
                },
                origin,
            ))
        }

        QueryableMethod::OrderByDesc | QueryableMethod::SortByDesc => {
            let key_expr = args.first().copied()?;
            let thir_key = arena.to_thir(key_expr);
            Some(alloc(
                arena,
                Plan::Sort {
                    input,
                    specs: vec![SortSpec {
                        key: SortKey::Expr(thir_key),
                        desc: true,
                    }],
                },
                origin,
            ))
        }

        // ── Distinct ───────────────────────────────────────────────────
        QueryableMethod::Distinct | QueryableMethod::Unique => Some(alloc(
            arena,
            Plan::Distinct {
                input,
                on: None,
            },
            origin,
        )),

        QueryableMethod::DistinctBy | QueryableMethod::UniqueBy => {
            let key_expr = args.first().copied()?;
            let thir_key = arena.to_thir(key_expr);
            Some(alloc(
                arena,
                Plan::Distinct {
                    input,
                    on: Some(vec![thir_key]),
                },
                origin,
            ))
        }

        // ── Limit / Skip / Take ────────────────────────────────────────
        QueryableMethod::Take => {
            let n = args.first().copied()?;
            let thir_n = arena.to_thir(n);
            Some(alloc(
                arena,
                Plan::Limit {
                    input,
                    skip: None,
                    fetch: Some(thir_n),
                },
                origin,
            ))
        }

        QueryableMethod::Skip => {
            let n = args.first().copied()?;
            let thir_n = arena.to_thir(n);
            Some(alloc(
                arena,
                Plan::Limit {
                    input,
                    skip: Some(thir_n),
                    fetch: None,
                },
                origin,
            ))
        }

        // ── Aggregates (scalar) ────────────────────────────────────────
        QueryableMethod::Sum => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Sum { expr: thir_expr }, interner, arena, origin))
        }
        QueryableMethod::Count => Some(scalar_agg(input, AggKind::Count, interner, arena, origin)),
        QueryableMethod::Avg => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Avg { expr: thir_expr }, interner, arena, origin))
        }
        QueryableMethod::Min => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Min { expr: thir_expr }, interner, arena, origin))
        }
        QueryableMethod::Max => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Max { expr: thir_expr }, interner, arena, origin))
        }

        // ── Aggregate with marker ──────────────────────────────────────
        QueryableMethod::Aggregate => {
            // .aggregate(Marker) — args[0] is the aggregate marker/impl
            let marker = args.first().copied()?;
            let thir_marker = arena.to_thir(marker);
            let output = interner.intern("_agg");
            Some(alloc(
                arena,
                Plan::Aggregate {
                    input,
                    keys: vec![],
                    aggs: vec![AggCall {
                        output,
                        kind: AggKind::UserAggregate {
                            // TODO: resolve the DefId of the Aggregate impl
                            impl_def: yelang_arena::DefId::new(1),
                            args: vec![thir_marker],
                            input_expr: None,
                            // TODO: resolve from trait impl's properties()
                            properties: crate::logical::plan::AggProperties {
                                class: crate::logical::plan::AggClass::Holistic,
                                associative: false,
                                commutative: false,
                                invertible: false,
                            },
                        },
                    }],
                    into: interner.intern("_groups"),
                },
                origin,
            ))
        }

        // ── Union ──────────────────────────────────────────────────────
        QueryableMethod::Union | QueryableMethod::UnionAll => {
            let other_expr = args.first().copied()?;
            let other = lower_expr_as_plan(other_expr, hir, interner, lang_items, arena)?;
            Some(alloc(
                arena,
                Plan::Union {
                    inputs: vec![input, other],
                },
                origin,
            ))
        }

        // ── Eager evaluation (fold/reduce/execute) ────────────────────
        // These are terminal operations that don't produce query plans.
        // They become opaque Extension barriers.
        QueryableMethod::Fold | QueryableMethod::Reduce | QueryableMethod::Execute => {
            Some(alloc(
                arena,
                Plan::Extension {
                    node: std::sync::Arc::new(OpaqueMethod {
                        name: method_name.to_string(),
                        input,
                        call: call_expr,
                    }),
                },
                origin,
            ))
        }
    }
}

/// Build a scalar aggregate (no grouping keys, single aggregate call).
fn scalar_agg(
    input: PlanId,
    kind: AggKind,
    interner: &Interner,
    arena: &mut PlanArena,
    origin: PlanOrigin,
) -> PlanId {
    let output = interner.intern("_result");
    alloc(
        arena,
        Plan::Aggregate {
            input,
            keys: vec![],
            aggs: vec![AggCall { output, kind }],
            into: interner.intern("_groups"),
        },
        origin,
    )
}

// ---------------------------------------------------------------------------
// Comprehension → Plan
// ---------------------------------------------------------------------------

fn lower_comprehension(
    element: ExprId,
    variables: &[ComprehensionVar],
    condition: Option<ExprId>,
    hir: &Crate,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    // A comprehension like `users@u[where u.age > 18][*].name` desugars to:
    //   Comprehension {
    //     element: <projection expr>,
    //     variables: [ComprehensionVar { source: users, flatten: 0, .. }],
    //     condition: Some(u.age > 18),
    //   }
    //
    // We build: Scan → Filter (if condition) → Map (element, flatten_depth).

    let var = variables.first()?;
    let origin = PlanOrigin::MethodCall(arena.to_thir(element));

    // Extract the source collection.
    let mut current = lower_expr_as_plan(var.source, hir, interner, lang_items, arena)?;

    // Apply the filter condition if present.
    if let Some(pred) = condition {
        let thir_pred = arena.to_thir(pred);
        current = alloc(
            arena,
            Plan::Filter { input: current, pred: thir_pred },
            origin.clone(),
        );
    }

    // Apply the projection with flatten depth.
    let thir_element = arena.to_thir(element);
    current = alloc(
        arena,
        Plan::Map {
            input: current,
            func: thir_element,
            flatten_depth: var.flatten,
        },
        origin,
    );

    Some(current)
}

// ---------------------------------------------------------------------------
// Intrinsic → Plan
// ---------------------------------------------------------------------------

fn lower_intrinsic(
    name: Symbol,
    args: &[ExprId],
    hir: &Crate,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let intrinsic_name = name.as_str(interner);
    let origin = PlanOrigin::Intrinsic(args.first().copied().map(|e| arena.to_thir(e)).unwrap_or_default());

    match intrinsic_name {
        // query_scan(table) → Scan
        "query_scan" => {
            let source_expr = args.first().copied()?;
            let thir_source = arena.to_thir(source_expr);
            Some(alloc(
                arena,
                Plan::Scan {
                    source: SourceRef::Call { func: thir_source },
                    filter: None,
                    projection: None,
                    range: None,
                },
                origin,
            ))
        }

        // query_filter(input, pred) → Filter
        "query_filter" => {
            let input_expr = args.first().copied()?;
            let pred = args.get(1).copied()?;
            let input = lower_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
            let thir_pred = arena.to_thir(pred);
            Some(alloc(
                arena,
                Plan::Filter { input, pred: thir_pred },
                origin,
            ))
        }

        // query_map(input, func) → Map
        "query_map" => {
            let input_expr = args.first().copied()?;
            let func = args.get(1).copied()?;
            let input = lower_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
            let thir_func = arena.to_thir(func);
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func: thir_func,
                    flatten_depth: 0,
                },
                origin,
            ))
        }

        // query_flat_map(input, func) → Map with flatten
        "query_flat_map" => {
            let input_expr = args.first().copied()?;
            let func = args.get(1).copied()?;
            let input = lower_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
            let thir_func = arena.to_thir(func);
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func: thir_func,
                    flatten_depth: 1,
                },
                origin,
            ))
        }

        // Unrecognized intrinsic: not a collection plan.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// OpaqueMethod — UserDefinedPlanNode for unrecognized methods
// ---------------------------------------------------------------------------

/// A user-defined or unrecognized Queryable method that acts as an
/// optimization barrier.
#[derive(Debug)]
struct OpaqueMethod {
    name: String,
    input: PlanId,
    #[allow(dead_code)]
    call: ExprId,
}

impl crate::logical::plan::UserDefinedPlanNode for OpaqueMethod {
    fn name(&self) -> &str {
        &self.name
    }

    fn inputs(&self) -> Vec<PlanId> {
        vec![self.input]
    }

    fn output_fields(&self) -> Vec<Symbol> {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// Correlated subquery extraction
// ---------------------------------------------------------------------------

/// Extract correlated subqueries from a projection expression.
///
/// Walks the THIR expression tree looking for `ThirExpr::Query` nodes.
/// For each nested query:
/// 1. Lowers it to a plan
/// 2. Computes outer references (symbols from the outer plan's output
///    that appear in the nested plan)
/// 3. If correlated (outer_refs non-empty), creates a DependentJoin
/// 4. Replaces the Query node with a column reference to the join's output
///
/// Returns the modified plan (with DependentJoins chained) and the
/// modified projection expression.
fn extract_correlated_subqueries(
    mut current: PlanId,
    projection: yelang_thir::ids::ThirExprId,
    thir_bodies: &ThirBodies,
    interner: &Interner,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> (PlanId, yelang_thir::ids::ThirExprId) {
    // Collect outer plan's output fields for correlation detection.
    let outer_fields = crate::analysis::plan_output_fields(arena.plan(current), arena);

    // Walk the projection expression and find Query nodes.
    let mut subquery_count = 0usize;
    let new_projection = extract_queries_from_expr(
        projection,
        &mut current,
        &outer_fields,
        thir_bodies,
        interner,
        arena,
        origin,
        &mut subquery_count,
    );

    (current, new_projection)
}

/// Recursively walk a THIR expression, extracting Query nodes.
fn extract_queries_from_expr(
    expr_id: yelang_thir::ids::ThirExprId,
    current: &mut PlanId,
    outer_fields: &yelang_arena::FxHashSet<Symbol>,
    thir_bodies: &ThirBodies,
    interner: &Interner,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
    subquery_count: &mut usize,
) -> yelang_thir::ids::ThirExprId {
    use yelang_thir::ThirExpr;

    let expr = match arena.thir_expr(expr_id).cloned() {
        Some(e) => e,
        None => return expr_id,
    };

    match &expr {
        // Found a nested query — extract it.
        ThirExpr::Query(select_query) => {
            // Lower the nested query to a plan.
            let nested_plan = lower_select(
                select_query,
                PlanOrigin::Synthetic,
                thir_bodies,
                interner,
                arena,
            );

            // Compute outer references: symbols from the outer plan
            // that appear in the nested plan.
            let nested_refs = crate::analysis::plan_referenced_fields(
                arena.plan(nested_plan),
                arena,
            );
            let outer_refs: Vec<Symbol> = nested_refs
                .iter()
                .filter(|s| outer_fields.contains(s))
                .copied()
                .collect();

            if outer_refs.is_empty() {
                // Not correlated — leave as a standalone scan.
                // The nested plan becomes a Constant or is inlined.
                // For now, just return the original expression.
                return expr_id;
            }

            // Correlated — create a DependentJoin.
            let output_col = interner.intern(&format!("_subquery_{}", *subquery_count));
            *subquery_count += 1;

            let dj = arena.alloc(Plan::DependentJoin {
                outer: *current,
                inner: nested_plan,
                pred: None, // Correlation predicate extracted during decorrelation.
                kind: crate::logical::plan::DepJoinKind::Single,
            });
            *current = dj;

            // Replace the Query node with a column reference.
            // Use Field { base, field: output_col } — the VM resolves by
            // field name from the join output row.
            let dummy_base = arena.alloc_thir_expr(ThirExpr::Literal(
                yelang_hir::hir::core::Lit::Unit,
            ));
            let col_ref = arena.alloc_thir_expr(ThirExpr::Field {
                base: dummy_base,
                field: output_col,
            });
            col_ref
        }

        // Recurse into binary expressions.
        ThirExpr::Binary { op, left, right } => {
            let new_left = extract_queries_from_expr(
                *left, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            let new_right = extract_queries_from_expr(
                *right, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            if new_left != *left || new_right != *right {
                arena.alloc_thir_expr(ThirExpr::Binary {
                    op: *op,
                    left: new_left,
                    right: new_right,
                })
            } else {
                expr_id
            }
        }

        // Recurse into unary expressions.
        ThirExpr::Unary { op, expr: inner } => {
            let new_inner = extract_queries_from_expr(
                *inner, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            if new_inner != *inner {
                arena.alloc_thir_expr(ThirExpr::Unary {
                    op: *op,
                    expr: new_inner,
                })
            } else {
                expr_id
            }
        }

        // Recurse into field access.
        ThirExpr::Field { base, field } => {
            let new_base = extract_queries_from_expr(
                *base, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            if new_base != *base {
                arena.alloc_thir_expr(ThirExpr::Field {
                    base: new_base,
                    field: *field,
                })
            } else {
                expr_id
            }
        }

        // Recurse into call arguments.
        ThirExpr::Call { func, args } => {
            let new_func = extract_queries_from_expr(
                *func, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            let new_args: Vec<_> = args
                .iter()
                .map(|&arg| {
                    extract_queries_from_expr(
                        arg, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
                    )
                })
                .collect();
            if new_func != *func || new_args != *args {
                arena.alloc_thir_expr(ThirExpr::Call {
                    func: new_func,
                    args: new_args,
                })
            } else {
                expr_id
            }
        }

        // Recurse into struct fields.
        ThirExpr::Struct { path, fields, rest } => {
            let new_fields: Vec<_> = fields
                .iter()
                .map(|&(name, expr)| {
                    let new_expr = extract_queries_from_expr(
                        expr, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
                    );
                    (name, new_expr)
                })
                .collect();
            let new_rest = rest.map(|r| {
                extract_queries_from_expr(
                    r, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
                )
            });
            let changed = new_fields != *fields || new_rest != *rest;
            if changed {
                arena.alloc_thir_expr(ThirExpr::Struct {
                    path: *path,
                    fields: new_fields,
                    rest: new_rest,
                })
            } else {
                expr_id
            }
        }

        // Recurse into tuple fields.
        ThirExpr::Tuple { fields } => {
            let new_fields: Vec<_> = fields
                .iter()
                .map(|&f| {
                    extract_queries_from_expr(
                        f, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
                    )
                })
                .collect();
            if new_fields != *fields {
                arena.alloc_thir_expr(ThirExpr::Tuple { fields: new_fields })
            } else {
                expr_id
            }
        }

        // Recurse into if condition.
        ThirExpr::If { cond, then_branch, else_branch } => {
            let new_cond = extract_queries_from_expr(
                *cond, current, outer_fields, thir_bodies, interner, arena, origin, subquery_count,
            );
            if new_cond != *cond {
                arena.alloc_thir_expr(ThirExpr::If {
                    cond: new_cond,
                    then_branch: *then_branch,
                    else_branch: *else_branch,
                })
            } else {
                expr_id
            }
        }

        // Leaf nodes and nodes we don't recurse into.
        _ => expr_id,
    }
}
