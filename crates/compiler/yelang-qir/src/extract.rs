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
use yelang_hir::hir::query::{
    FromNode, GroupByClause, OrderByPart, QueryKind, SelectLinkPath,
    SelectLinkSegment, SelectQuery,
};
use yelang_hir::ids::{PatId, QueryId};
use yelang_hir::Crate;
use yelang_interner::{Interner, Symbol};

use crate::plan::{
    Direction, EdgeRef, ExprRef, JoinKind, NodeRef, OrderSpec, Plan, PlanArena, PlanId,
    PlanOrigin, PlanRange, SourceRef, TraversePath, TraverseSegment,
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
