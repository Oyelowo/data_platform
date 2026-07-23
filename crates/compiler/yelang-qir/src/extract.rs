//! Plan extraction — builds a [`PlanArena`] from HIR query nodes.
//!
//! The main entry point is [`extract_query`], which dispatches on
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

use crate::plan::{
    AggCall, AggKind, Direction, EdgeRef, GroupKey, JoinKind, NodeRef, Plan, PlanArena,
    PlanId, PlanOrigin, PlanRange, SortKey, SortSpec, SourceRef, TraversePath, TraverseSegment,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract a logical plan tree from a HIR query.
///
/// Returns the root [`PlanId`] of the extracted tree, allocated in `arena`.
pub fn extract_query(
    query_id: QueryId,
    hir: &Crate,
    interner: &Interner,
    lang_items: &LangItems,
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let query = hir.query(query_id)?;
    match &query.kind {
        QueryKind::Select(select) => {
            Some(extract_select(select, query_id, hir, interner, lang_items, arena))
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
// Select extraction
// ---------------------------------------------------------------------------

fn extract_select(
    select: &SelectQuery,
    query_id: QueryId,
    hir: &Crate,
    interner: &Interner,
    _lang_items: &LangItems,
    arena: &mut PlanArena,
) -> PlanId {
    let origin = PlanOrigin::QuerySyntax(query_id);

    // 1. Build scan(s) from `from` nodes.
    let mut current = extract_from_nodes(&select.from, hir, arena, &origin);

    // 2. Apply `links` traversals.
    if !select.links.is_empty() {
        let paths = extract_link_paths(&select.links, hir, interner, arena);
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
        current = extract_group_by(current, group_by, interner, arena, &origin);
    }

    // 5. Apply `order by`.
    if !select.order_by.is_empty() {
        let specs = extract_order_specs(&select.order_by, arena);
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

fn extract_from_nodes(
    from: &[FromNode],
    hir: &Crate,
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    debug_assert!(!from.is_empty(), "select must have at least one from node");

    let mut scans: Vec<PlanId> = Vec::with_capacity(from.len());

    for node in from {
        let mut scan_id = extract_single_from(node, hir, arena, origin);

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
            let specs = extract_order_specs(&node.order_by, arena);
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

fn extract_single_from(
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

fn extract_link_paths(
    links: &[SelectLinkPath],
    hir: &Crate,
    interner: &Interner,
    arena: &PlanArena,
) -> Vec<TraversePath> {
    links
        .iter()
        .map(|path| extract_link_path(path, hir, interner, arena))
        .collect()
}

fn extract_link_path(
    path: &SelectLinkPath,
    hir: &Crate,
    interner: &Interner,
    arena: &PlanArena,
) -> TraversePath {
    let anchor = path.start.var.symbol;

    let segments = path
        .segments
        .iter()
        .map(|seg| extract_link_segment(seg, hir, interner, arena))
        .collect();

    TraversePath { anchor, segments }
}

fn extract_link_segment(
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

fn extract_group_by(
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

            let kind = match name {
                "sum" => Some(AggKind::Sum { expr: arena.to_thir(expr) }),
                "count" => Some(AggKind::Count),
                "avg" => Some(AggKind::Avg { expr: arena.to_thir(expr) }),
                "min" => Some(AggKind::Min { expr: arena.to_thir(expr) }),
                "max" => Some(AggKind::Max { expr: arena.to_thir(expr) }),
                "aggregate" => {
                    // User-defined aggregate via .aggregate(Marker).
                    let marker = args.first().copied();
                    Some(AggKind::UserAggregate {
                        impl_def: yelang_arena::DefId::new(1), // TODO: resolve
                        args: marker.map(|m| vec![arena.to_thir(m)]).unwrap_or_default(),
                        input_expr: None,
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

fn extract_order_specs(parts: &[OrderByPart], arena: &PlanArena) -> Vec<SortSpec> {
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
/// [`extract_query`]). It handles:
/// - `Queryable` method chains: `.filter()`, `.map()`, `.join()`, …
/// - Comprehensions (desugared selectors): `[*]`, `[where …]`, `[**]`
/// - `@intrinsic(query_*)` calls
/// - Table/path references → `Scan`
/// - Nested `select` queries → recursive [`extract_query`]
///
/// Returns `None` if the expression cannot be interpreted as a collection
/// plan (e.g. a scalar literal, a non-Queryable function call).
pub fn extract_expr_as_plan(
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
                extract_method_call(expr_id, *receiver, method.symbol, args, hir, interner, lang_items, arena)
            } else {
                // Non-Queryable method: treat as opaque extension.
                let input = extract_expr_as_plan(*receiver, hir, interner, lang_items, arena);
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
        } => extract_comprehension(*element, variables, *condition, hir, interner, lang_items, arena),

        // ── Intrinsic call ─────────────────────────────────────────────
        Expr::Intrinsic { name, args } => {
            extract_intrinsic(name.symbol, args, hir, interner, lang_items, arena)
        }

        // ── Nested select query ────────────────────────────────────────
        Expr::Query(query_id) => extract_query(*query_id, hir, interner, lang_items, arena),

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

fn extract_method_call(
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
    let input = extract_expr_as_plan(receiver, hir, interner, lang_items, arena)?;

    match method_name {
        // ── Filter ─────────────────────────────────────────────────────
        "filter" => {
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
        "map" => {
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
        "flat_map" => {
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
        "join" | "inner_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
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

        "left_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
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

        "semi_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
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

        "anti_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, lang_items, arena)?;
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
        "group_by" => {
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
        "order_by" | "sort_by" => {
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

        "order_by_desc" | "sort_by_desc" => {
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
        "distinct" | "unique" => Some(alloc(
            arena,
            Plan::Distinct {
                input,
                on: None,
            },
            origin,
        )),

        "distinct_by" | "unique_by" => {
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
        "take" => {
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

        "skip" => {
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
        "sum" => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Sum { expr: thir_expr }, interner, arena, origin))
        }
        "count" => Some(scalar_agg(input, AggKind::Count, interner, arena, origin)),
        "avg" => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Avg { expr: thir_expr }, interner, arena, origin))
        }
        "min" => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Min { expr: thir_expr }, interner, arena, origin))
        }
        "max" => {
            let thir_expr = arena.to_thir(call_expr);
            Some(scalar_agg(input, AggKind::Max { expr: thir_expr }, interner, arena, origin))
        }

        // ── Aggregate with marker ──────────────────────────────────────
        "aggregate" => {
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
                        },
                    }],
                    into: interner.intern("_groups"),
                },
                origin,
            ))
        }

        // ── Union ──────────────────────────────────────────────────────
        "union" | "union_all" => {
            let other_expr = args.first().copied()?;
            let other = extract_expr_as_plan(other_expr, hir, interner, lang_items, arena)?;
            Some(alloc(
                arena,
                Plan::Union {
                    inputs: vec![input, other],
                },
                origin,
            ))
        }

        // ── Unknown method: opaque barrier ─────────────────────────────
        _ => Some(alloc(
            arena,
            Plan::Extension {
                node: std::sync::Arc::new(OpaqueMethod {
                    name: method_name.to_string(),
                    input,
                    call: call_expr,
                }),
            },
            origin,
        )),
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

fn extract_comprehension(
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
    let mut current = extract_expr_as_plan(var.source, hir, interner, lang_items, arena)?;

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

fn extract_intrinsic(
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
            let input = extract_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
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
            let input = extract_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
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
            let input = extract_expr_as_plan(input_expr, hir, interner, lang_items, arena)?;
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

impl crate::plan::UserDefinedPlanNode for OpaqueMethod {
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
