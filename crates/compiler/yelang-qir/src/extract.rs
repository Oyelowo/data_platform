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

use crate::plan::{
    AggCall, AggKind, Direction, EdgeRef, ExprRef, JoinKind, NodeRef, OrderSpec, Plan, PlanArena,
    PlanId, PlanOrigin, PlanRange, SourceRef, TraversePath, TraverseSegment,
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
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let query = hir.query(query_id)?;
    match &query.kind {
        QueryKind::Select(select) => Some(extract_select(select, query_id, hir, interner, arena)),
        // Mutations are not yet handled by the logical plan.
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
    arena: &mut PlanArena,
) -> PlanId {
    let origin = PlanOrigin::QuerySyntax(query_id);

    // 1. Build scan(s) from `from` nodes.
    let mut current = extract_from_nodes(&select.from, arena, &origin);

    // 2. Apply `links` traversals.
    if !select.links.is_empty() {
        let paths = extract_link_paths(&select.links, hir, interner);
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
        current = alloc(
            arena,
            Plan::Filter { input: current, pred },
            origin.clone(),
        );
    }

    // 4. Apply `group by`.
    if let Some(group_by) = &select.group_by {
        current = extract_group_by(current, group_by, interner, arena, &origin);
    }

    // 5. Apply `order by`.
    if !select.order_by.is_empty() {
        let specs = extract_order_specs(&select.order_by);
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

    // 7. Apply projection.
    let result_name = interner.intern("result");
    current = alloc(
        arena,
        Plan::Project {
            input: current,
            exprs: vec![(result_name, select.projection)],
        },
        origin,
    );

    current
}

// ---------------------------------------------------------------------------
// From nodes → Scan (+ optional Filter, Sort, Limit)
// ---------------------------------------------------------------------------

fn extract_from_nodes(
    from: &[FromNode],
    arena: &mut PlanArena,
    origin: &PlanOrigin,
) -> PlanId {
    debug_assert!(!from.is_empty(), "select must have at least one from node");

    let mut scans: Vec<PlanId> = Vec::with_capacity(from.len());

    for node in from {
        let mut scan_id = extract_single_from(node, arena, origin);

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
            let specs = extract_order_specs(&node.order_by);
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

fn extract_single_from(node: &FromNode, arena: &mut PlanArena, origin: &PlanOrigin) -> PlanId {
    let source = SourceRef::Local { name: node.label };

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

// ---------------------------------------------------------------------------
// Links → TraversePath
// ---------------------------------------------------------------------------

fn extract_link_paths(
    links: &[SelectLinkPath],
    hir: &Crate,
    interner: &Interner,
) -> Vec<TraversePath> {
    links
        .iter()
        .map(|path| extract_link_path(path, hir, interner))
        .collect()
}

fn extract_link_path(
    path: &SelectLinkPath,
    hir: &Crate,
    interner: &Interner,
) -> TraversePath {
    let anchor = path.start.var.symbol;

    let segments = path
        .segments
        .iter()
        .map(|seg| extract_link_segment(seg, hir, interner))
        .collect();

    TraversePath { anchor, segments }
}

fn extract_link_segment(
    seg: &SelectLinkSegment,
    hir: &Crate,
    _interner: &Interner,
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
        edge_pred: seg.edge.modifiers.filter,
        target_pred: seg.target.modifiers.filter,
        hop_range: seg.edge.hops.as_ref().map(|h| PlanRange {
            start: h.start,
            end: h.end,
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
    let keys: Vec<(Symbol, ExprRef)> = group_by
        .keys
        .iter()
        .map(|key| {
            let name = key.name.map(|ident| ident.symbol).unwrap_or(fallback);
            (name, key.expr)
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
// Order by → OrderSpec
// ---------------------------------------------------------------------------

fn extract_order_specs(parts: &[OrderByPart]) -> Vec<OrderSpec> {
    parts
        .iter()
        .map(|part| OrderSpec {
            expr: part.expr,
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
    // TODO: inspect the pattern kind in the HIR pattern arena to extract
    // the identifier symbol. For now, return a placeholder.
    let _ = (pat_id, hir);
    Symbol::from(1u32)
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
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let expr = hir.expr(expr_id)?;
    let origin = PlanOrigin::MethodCall(expr_id);

    match expr {
        // ── Queryable method call ──────────────────────────────────────
        Expr::MethodCall {
            receiver,
            method,
            args,
            trait_def_id: _,
        } => extract_method_call(expr_id, *receiver, method.symbol, args, hir, interner, arena),

        // ── Comprehension (desugared selector) ─────────────────────────
        Expr::Comprehension {
            kind: ComprehensionKind::List,
            element,
            variables,
            condition,
        } => extract_comprehension(*element, variables, *condition, hir, interner, arena),

        // ── Intrinsic call ─────────────────────────────────────────────
        Expr::Intrinsic { name, args } => {
            extract_intrinsic(name.symbol, args, hir, interner, arena)
        }

        // ── Nested select query ────────────────────────────────────────
        Expr::Query(query_id) => extract_query(*query_id, hir, interner, arena),

        // ── Path reference (table or local variable) ───────────────────
        Expr::Path { res: _ } => {
            // A path to a table or collection variable → Scan.
            // TODO: resolve the DefId to determine if it's a @table struct.
            let name = Symbol::from(1u32); // placeholder
            Some(alloc(
                arena,
                Plan::Scan {
                    source: SourceRef::Local { name },
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
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let origin = PlanOrigin::MethodCall(call_expr);
    let method_name = method.as_str(interner);

    // Extract the receiver as a plan (the collection being operated on).
    let input = extract_expr_as_plan(receiver, hir, interner, arena)?;

    match method_name {
        // ── Filter ─────────────────────────────────────────────────────
        "filter" => {
            // .filter(|x| pred) — args[0] is the closure
            let pred = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Filter { input, pred },
                origin,
            ))
        }

        // ── Map ────────────────────────────────────────────────────────
        "map" => {
            // .map(|x| expr) — args[0] is the closure
            let func = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func,
                    flatten_depth: 0,
                },
                origin,
            ))
        }

        // ── FlatMap ────────────────────────────────────────────────────
        "flat_map" => {
            let func = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func,
                    flatten_depth: 1,
                },
                origin,
            ))
        }

        // ── Joins ──────────────────────────────────────────────────────
        "join" | "inner_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Inner,
                    on: vec![], // TODO: decompose the closure into equi-join keys
                    filter: Some(on_expr),
                },
                origin,
            ))
        }

        "left_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Left,
                    on: vec![],
                    filter: Some(on_expr),
                },
                origin,
            ))
        }

        "semi_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Semi,
                    on: vec![],
                    filter: Some(on_expr),
                },
                origin,
            ))
        }

        "anti_join" => {
            let right_expr = args.first().copied()?;
            let on_expr = args.get(1).copied()?;
            let right = extract_expr_as_plan(right_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Join {
                    left: input,
                    right,
                    kind: JoinKind::Anti,
                    on: vec![],
                    filter: Some(on_expr),
                },
                origin,
            ))
        }

        // ── Group by ───────────────────────────────────────────────────
        "group_by" => {
            // .group_by(|x| key_expr) — args[0] is the key closure
            let key_expr = args.first().copied()?;
            let key_name = interner.intern("_key");
            let into = interner.intern("_groups");
            Some(alloc(
                arena,
                Plan::Aggregate {
                    input,
                    keys: vec![(key_name, key_expr)],
                    aggs: vec![],
                    into,
                },
                origin,
            ))
        }

        // ── Order by ───────────────────────────────────────────────────
        "order_by" | "sort_by" => {
            let key_expr = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Sort {
                    input,
                    specs: vec![OrderSpec {
                        expr: key_expr,
                        desc: false,
                    }],
                },
                origin,
            ))
        }

        "order_by_desc" | "sort_by_desc" => {
            let key_expr = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Sort {
                    input,
                    specs: vec![OrderSpec {
                        expr: key_expr,
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
            Some(alloc(
                arena,
                Plan::Distinct {
                    input,
                    on: Some(vec![key_expr]),
                },
                origin,
            ))
        }

        // ── Limit / Skip / Take ────────────────────────────────────────
        "take" => {
            let n = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Limit {
                    input,
                    skip: None,
                    fetch: Some(n),
                },
                origin,
            ))
        }

        "skip" => {
            let n = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Limit {
                    input,
                    skip: Some(n),
                    fetch: None,
                },
                origin,
            ))
        }

        // ── Aggregates (scalar) ────────────────────────────────────────
        "sum" => Some(scalar_agg(input, AggKind::Sum { expr: call_expr }, interner, arena, origin)),
        "count" => Some(scalar_agg(input, AggKind::Count, interner, arena, origin)),
        "avg" => Some(scalar_agg(input, AggKind::Avg { expr: call_expr }, interner, arena, origin)),
        "min" => Some(scalar_agg(input, AggKind::Min { expr: call_expr }, interner, arena, origin)),
        "max" => Some(scalar_agg(input, AggKind::Max { expr: call_expr }, interner, arena, origin)),

        // ── Aggregate with marker ──────────────────────────────────────
        "aggregate" => {
            // .aggregate(Marker) — args[0] is the aggregate marker/impl
            let marker = args.first().copied()?;
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
                            args: vec![marker],
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
            let other = extract_expr_as_plan(other_expr, hir, interner, arena)?;
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
    let origin = PlanOrigin::MethodCall(element);

    // Extract the source collection.
    let mut current = extract_expr_as_plan(var.source, hir, interner, arena)?;

    // Apply the filter condition if present.
    if let Some(pred) = condition {
        current = alloc(
            arena,
            Plan::Filter { input: current, pred },
            origin.clone(),
        );
    }

    // Apply the projection with flatten depth.
    current = alloc(
        arena,
        Plan::Map {
            input: current,
            func: element,
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
    arena: &mut PlanArena,
) -> Option<PlanId> {
    let intrinsic_name = name.as_str(interner);
    let origin = PlanOrigin::Intrinsic(args.first().copied().unwrap_or_else(|| {
        // Fallback: use a dummy ExprId. This shouldn't happen in practice.
        yelang_hir::ids::ExprId::default()
    }));

    match intrinsic_name {
        // query_scan(table) → Scan
        "query_scan" => {
            let source_expr = args.first().copied()?;
            Some(alloc(
                arena,
                Plan::Scan {
                    source: SourceRef::Call { func: source_expr },
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
            let input = extract_expr_as_plan(input_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Filter { input, pred },
                origin,
            ))
        }

        // query_map(input, func) → Map
        "query_map" => {
            let input_expr = args.first().copied()?;
            let func = args.get(1).copied()?;
            let input = extract_expr_as_plan(input_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func,
                    flatten_depth: 0,
                },
                origin,
            ))
        }

        // query_flat_map(input, func) → Map with flatten
        "query_flat_map" => {
            let input_expr = args.first().copied()?;
            let func = args.get(1).copied()?;
            let input = extract_expr_as_plan(input_expr, hir, interner, arena)?;
            Some(alloc(
                arena,
                Plan::Map {
                    input,
                    func,
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
