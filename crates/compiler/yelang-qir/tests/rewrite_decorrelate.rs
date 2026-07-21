//! The §4.2 pushdown rule table, one family per case. Phase J3 (exit tests T-DECOR-01…30).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §4; cases from PHASEJ_QIR_CHECKLIST.md (Decorrelation matrix).
//!
//! T-DECOR-01 filter pushdown
//! T-DECOR-02 map/repr extension
//! T-DECOR-03 inner join both-side accessing
//! T-DECOR-04 semi/anti
//! T-DECOR-05 mark join (EXISTS shape)
//! T-DECOR-06 left-outer null-rejecting vs not
//! T-DECOR-07 full-outer IS NOT DISTINCT FROM
//! T-DECOR-08 static agg empty-input guard (COUNT bug)
//! T-DECOR-09 grouped agg Γ rule
//! T-DECOR-10 window PARTITION BY amendment
//! T-DECOR-11 order-only pushdown
//! T-DECOR-12 per-group TopN
//! T-DECOR-13 ROW_NUMBER fallback
//! T-DECOR-14 OFFSET through both routes
//! T-DECOR-15 set-op branches
//! T-DECOR-16 distinct
//! T-DECOR-17 OneRow/constants
//! T-DECOR-18 nested 2-level with shared outer refs (state merge; bounded memory — the BTW'25 blow-up shape)
//! T-DECOR-19 union-find substitution drops D-join
//! T-DECOR-20 keep-D-join when selective (cost flag)
//! T-DECOR-21 correlated links traversal
//! T-DECOR-22 scalar subquery in projection
//! T-DECOR-23 EXISTS/NOT EXISTS
//! T-DECOR-24 `IN` (semi) and `NOT IN` with Option semantics (anti + three-valued guard -> compile error or explicit)
//! T-DECOR-25 correlated aggregate in HAVING position
//! T-DECOR-26 decorrelation after elision ordering (pass order fixed, documented)
//! T-DECOR-27 post-pass assert: no DependentJoin/Subplan remains
//! T-DECOR-28..30 differential: every case above against reference executor on random data
