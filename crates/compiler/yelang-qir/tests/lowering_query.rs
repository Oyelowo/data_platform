//! Golden LIR per surface feature. Phase J1 (exit tests T-LOW-01…20).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §9; cases from PHASEJ_QIR_CHECKLIST.md (Lowering matrix).
//!
//! T-LOW-01 select/from/where/order/range golden LIR
//! T-LOW-02 multi-root from
//! T-LOW-03 links traversal (1..n hops, directions)
//! T-LOW-04 group-by group records
//! T-LOW-05 nested projection -> Subplan+DependentJoin
//! T-LOW-06 object projection shorthand/rename/computed
//! T-LOW-07 comprehension [*]/[**]/[where]
//! T-LOW-08 let-bound source inlining
//! T-LOW-09 method chain -> same LIR as clause form
//! T-LOW-10 mutation queries: structured result or structured error
//! T-LOW-11 Seq/Coll types on every node
//! T-LOW-12 `execute()` boundary
//! T-LOW-13 `select 1` -> Expr root with dead scan marked
//! T-LOW-14 binder shadowing
//! T-LOW-15 datetime/other literals
//! T-LOW-16 Option-typed fields through joins
//! T-LOW-17 empty from-collection edge cases
//! T-LOW-18 deeply nested queries (depth 8)
//! T-LOW-19 query in fn args/returns
//! T-LOW-20 query over Querying adapter
