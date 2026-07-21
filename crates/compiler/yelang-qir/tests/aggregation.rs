//! Classes x partial/final x empty-input x distinct. Phase J4 (exit tests T-AGG-11…24).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §5; cases from PHASEJ_QIR_CHECKLIST.md (Aggregates matrix).
//!
//! T-AGG-11 class read from `class()` body (not name)
//! T-AGG-12 config aggregate (Percentile{p}) config shipped to closures
//! T-AGG-13 distributive partial+final == full (random partitions)
//! T-AGG-14 algebraic (avg) partial+final == full
//! T-AGG-15 holistic forces repartition (plan assertion)
//! T-AGG-16 shared hash table across mixed-class group-by (plan assertion)
//! T-AGG-17 count(distinct x)
//! T-AGG-18 aggregate in projection of grouped query
//! T-AGG-19 aggregate over nested collection (per-group members)
//! T-AGG-20 aggregate estimates present in stats
//! T-AGG-21 volatility: volatile fn inside per_row rejected for remote, allowed local
//! T-AGG-22 `sum(u)` error path
//! T-AGG-23 generic UDA impl (Count over T) extraction
//! T-AGG-24 UDA with user-defined Acc struct type
