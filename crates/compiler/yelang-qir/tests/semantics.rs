//! Semantics invariants: projection-determines-shape, elision, order honesty. Phase J0–J5 (T-SEM-01…10).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §2.4–§2.5; cases from PHASEJ_QIR_CHECKLIST.md
//! (Semantics invariants matrix).
//!
//! T-SEM-01 projection determines shape: `select 1` == 1; `select [1,2]` == [1,2]
//! T-SEM-02 group-by variant of 01
//! T-SEM-03 links variant of 01
//! T-SEM-04 range doesn't change shape
//! T-SEM-05 projection typing: query type == projection type (tycheck unit)
//! T-SEM-06 elision preserves semantics
//! T-SEM-07 no implicit row mapping (SQL-style `SELECT u.*` has no equivalent — error)
//! T-SEM-08 aggregate-free projection never aggregates
//! T-SEM-09 element binders never leak to projection scope (compile-fail suite)
//! T-SEM-10 `from` required (rootless select parse error)
