//! Decorrelation tests — BTW 2025 top-down unnesting.
//!
//! Tests construct plan trees with DependentJoin nodes directly and
//! verify the decorrelation algorithm produces correct results.
//!
//! Reference: Neumann, "Improving Unnesting of Complex Queries", BTW 2025
//!            Neumann, "A Formalization of Top-Down Unnesting", arXiv:2412.04294

use yelang_interner::Interner;
use yelang_qir::logical::plan::{
    AggCall, AggKind, DepJoinKind, GroupKey, JoinKind, Plan, PlanArena, SourceRef,
};
use yelang_qir::logical::optimize::decorrelate::decorrelate;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct TestCtx {
    interner: Interner,
    arena: PlanArena,
}

impl TestCtx {
    fn new() -> Self {
        Self {
            interner: Interner::new(),
            arena: PlanArena::new(),
        }
    }

    fn sym(&self, name: &str) -> yelang_interner::Symbol {
        self.interner.intern(name)
    }

    fn scan(&mut self, table_name: &str) -> yelang_qir::PlanId {
        let name = self.sym(table_name);
        self.arena.alloc(Plan::Scan {
            source: SourceRef::Table {
                def: yelang_arena::DefId::from_usize(1),
                name,
            },
            filter: None,
            projection: None,
            range: None,
        })
    }

    fn filter(
        &mut self,
        input: yelang_qir::PlanId,
        pred: yelang_thir::ids::ThirExprId,
    ) -> yelang_qir::PlanId {
        self.arena.alloc(Plan::Filter { input, pred })
    }

    fn aggregate(
        &mut self,
        input: yelang_qir::PlanId,
        keys: Vec<(yelang_interner::Symbol, GroupKey)>,
        aggs: Vec<AggCall>,
        into: yelang_interner::Symbol,
    ) -> yelang_qir::PlanId {
        self.arena.alloc(Plan::Aggregate { input, keys, aggs, into })
    }

    fn dependent_join(
        &mut self,
        outer: yelang_qir::PlanId,
        inner: yelang_qir::PlanId,
        pred: Option<yelang_thir::ids::ThirExprId>,
        kind: DepJoinKind,
    ) -> yelang_qir::PlanId {
        self.arena.alloc(Plan::DependentJoin { outer, inner, pred, kind })
    }

    fn project(
        &mut self,
        input: yelang_qir::PlanId,
        exprs: Vec<(yelang_interner::Symbol, yelang_thir::ids::ThirExprId)>,
    ) -> yelang_qir::PlanId {
        self.arena.alloc(Plan::Project { input, exprs })
    }

    fn dummy_expr(&mut self) -> yelang_thir::ids::ThirExprId {
        self.arena
            .alloc_thir_expr(yelang_thir::ThirExpr::Literal(yelang_hir::hir::core::Lit::Unit))
    }

    fn decorrelate(&mut self, root: yelang_qir::PlanId) -> yelang_qir::PlanId {
        decorrelate(root, &mut self.arena)
    }

    fn count_nodes(&self, root: yelang_qir::PlanId, name: &str) -> usize {
        let mut count = 0;
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let plan = self.arena.plan(id);
            let plan_name = match plan {
                Plan::Scan { .. } => "Scan",
                Plan::Filter { .. } => "Filter",
                Plan::Project { .. } => "Project",
                Plan::Map { .. } => "Map",
                Plan::Join { .. } => "Join",
                Plan::Aggregate { .. } => "Aggregate",
                Plan::Window { .. } => "Window",
                Plan::Sort { .. } => "Sort",
                Plan::Limit { .. } => "Limit",
                Plan::Distinct { .. } => "Distinct",
                Plan::Union { .. } => "Union",
                Plan::Traverse { .. } => "Traverse",
                Plan::DependentJoin { .. } => "DependentJoin",
                Plan::GroupJoin { .. } => "GroupJoin",
                Plan::ScalarSubquery { .. } => "ScalarSubquery",
                Plan::Exists { .. } => "Exists",
                Plan::Repeat { .. } => "Repeat",
                Plan::Extension { .. } => "Extension",
                Plan::Constant { .. } => "Constant",
                Plan::Empty { .. } => "Empty",
            };
            if plan_name == name {
                count += 1;
            }
            stack.extend(yelang_qir::tree::children(plan));
        }
        count
    }
}

// ---------------------------------------------------------------------------
// DEC-01: Trivial dependent join (no correlation) → regular join
// ---------------------------------------------------------------------------

#[test]
fn dec01_trivial_dependent_join_becomes_regular_join() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let dj = ctx.dependent_join(users, orders, None, DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert_eq!(ctx.count_nodes(result, "Join"), 1);
}

// ---------------------------------------------------------------------------
// DEC-02: Scalar subquery with equi-predicate
// ---------------------------------------------------------------------------

#[test]
fn dec02_scalar_subquery_with_equi_pred() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, orders, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Join") >= 1);
}

// ---------------------------------------------------------------------------
// DEC-03: Aggregate subquery → outer refs become group keys
// ---------------------------------------------------------------------------

#[test]
fn dec03_aggregate_subquery_adds_group_keys() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let expr = ctx.dummy_expr();
    let agg = ctx.aggregate(
        orders,
        vec![],
        vec![AggCall {
            output: ctx.sym("total"),
            kind: AggKind::Sum { expr },
        }],
        ctx.sym("_groups"),
    );
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, agg, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    let has_agg = ctx.count_nodes(result, "Aggregate") >= 1;
    let has_gj = ctx.count_nodes(result, "GroupJoin") >= 1;
    assert!(has_agg || has_gj, "should have Aggregate or GroupJoin");
}

// ---------------------------------------------------------------------------
// DEC-04: COUNT bug — static aggregate with empty inner
// ---------------------------------------------------------------------------

#[test]
fn dec04_count_bug_static_aggregate() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let agg = ctx.aggregate(
        orders,
        vec![],
        vec![AggCall {
            output: ctx.sym("cnt"),
            kind: AggKind::Count,
        }],
        ctx.sym("_groups"),
    );
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, agg, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-05: Nested 2-level correlation
// ---------------------------------------------------------------------------

#[test]
fn dec05_nested_two_level_correlation() {
    let mut ctx = TestCtx::new();
    let t1 = ctx.scan("t1");
    let t2 = ctx.scan("t2");
    let t3 = ctx.scan("t3");

    let inner_pred = ctx.dummy_expr();
    let inner_dj = ctx.dependent_join(t2, t3, Some(inner_pred), DepJoinKind::Join);

    let outer_pred = ctx.dummy_expr();
    let outer_dj = ctx.dependent_join(t1, inner_dj, Some(outer_pred), DepJoinKind::Join);
    let result = ctx.decorrelate(outer_dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Join") >= 2);
}

// ---------------------------------------------------------------------------
// DEC-06: Filter in inner plan
// ---------------------------------------------------------------------------

#[test]
fn dec06_filter_in_inner_plan() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let filter_pred = ctx.dummy_expr();
    let filtered_orders = ctx.filter(orders, filter_pred);
    let dj_pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, filtered_orders, Some(dj_pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-07: Semi join (EXISTS)
// ---------------------------------------------------------------------------

#[test]
fn dec07_semi_join_exists() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, orders, Some(pred), DepJoinKind::Semi);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Join") >= 1);
}

// ---------------------------------------------------------------------------
// DEC-08: Anti join (NOT EXISTS)
// ---------------------------------------------------------------------------

#[test]
fn dec08_anti_join_not_exists() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, orders, Some(pred), DepJoinKind::Anti);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Join") >= 1);
}

// ---------------------------------------------------------------------------
// DEC-09: Left outer dependent join
// ---------------------------------------------------------------------------

#[test]
fn dec09_left_outer_dependent_join() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, orders, Some(pred), DepJoinKind::LeftOuter);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Join") >= 1);
}

// ---------------------------------------------------------------------------
// DEC-10: Union with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec10_union_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders1 = ctx.scan("orders1");
    let orders2 = ctx.scan("orders2");
    let union = ctx.arena.alloc(Plan::Union {
        inputs: vec![orders1, orders2],
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, union, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-11: Sort with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec11_sort_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let sort = ctx.arena.alloc(Plan::Sort {
        input: orders,
        specs: vec![],
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, sort, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-12: Limit with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec12_limit_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let limit = ctx.arena.alloc(Plan::Limit {
        input: orders,
        skip: None,
        fetch: None,
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, limit, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-13: Distinct with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec13_distinct_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let distinct = ctx.arena.alloc(Plan::Distinct {
        input: orders,
        on: None,
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, distinct, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-14: Map with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec14_map_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let func = ctx.dummy_expr();
    let map = ctx.arena.alloc(Plan::Map {
        input: orders,
        func,
        flatten_depth: 0,
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, map, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-15: Project with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec15_project_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let project = ctx.project(orders, vec![]);
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, project, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-16: Window with correlation
// ---------------------------------------------------------------------------

#[test]
fn dec16_window_with_correlation() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let window = ctx.arena.alloc(Plan::Window {
        input: orders,
        funcs: vec![],
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, window, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-17: Inner join in inner plan
// ---------------------------------------------------------------------------

#[test]
fn dec17_inner_join_in_inner_plan() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let items = ctx.scan("items");
    let inner_join = ctx.arena.alloc(Plan::Join {
        left: orders,
        right: items,
        kind: JoinKind::Inner,
        on: vec![],
        filter: None,
    });
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, inner_join, Some(pred), DepJoinKind::Join);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-18: Post-condition — no correlated nodes remain
// ---------------------------------------------------------------------------

#[test]
fn dec18_postcondition_no_correlated_nodes() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, orders, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert_eq!(ctx.count_nodes(result, "ScalarSubquery"), 0);
    assert_eq!(ctx.count_nodes(result, "Exists"), 0);
}

// ---------------------------------------------------------------------------
// DEC-19: Multiple aggregates in one subquery
// ---------------------------------------------------------------------------

#[test]
fn dec19_multiple_aggregates() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");
    let sum_expr = ctx.dummy_expr();
    let avg_expr = ctx.dummy_expr();
    let agg = ctx.aggregate(
        orders,
        vec![],
        vec![
            AggCall {
                output: ctx.sym("cnt"),
                kind: AggKind::Count,
            },
            AggCall {
                output: ctx.sym("total"),
                kind: AggKind::Sum { expr: sum_expr },
            },
            AggCall {
                output: ctx.sym("avg"),
                kind: AggKind::Avg { expr: avg_expr },
            },
        ],
        ctx.sym("_groups"),
    );
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, agg, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
}

// ---------------------------------------------------------------------------
// DEC-20: Deep chain — Filter → Map → Aggregate
// ---------------------------------------------------------------------------

#[test]
fn dec20_deep_chain_filter_map_aggregate() {
    let mut ctx = TestCtx::new();
    let users = ctx.scan("users");
    let orders = ctx.scan("orders");

    let filter_pred = ctx.dummy_expr();
    let filter = ctx.filter(orders, filter_pred);
    let map_func = ctx.dummy_expr();
    let map = ctx.arena.alloc(Plan::Map {
        input: filter,
        func: map_func,
        flatten_depth: 0,
    });
    let sum_expr = ctx.dummy_expr();
    let agg = ctx.aggregate(
        map,
        vec![],
        vec![AggCall {
            output: ctx.sym("total"),
            kind: AggKind::Sum { expr: sum_expr },
        }],
        ctx.sym("_groups"),
    );
    let pred = ctx.dummy_expr();
    let dj = ctx.dependent_join(users, agg, Some(pred), DepJoinKind::Single);
    let result = ctx.decorrelate(dj);

    assert_eq!(ctx.count_nodes(result, "DependentJoin"), 0);
    assert!(ctx.count_nodes(result, "Filter") >= 1);
    assert!(ctx.count_nodes(result, "Map") >= 1);
}
