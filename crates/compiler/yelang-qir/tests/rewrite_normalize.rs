//! Normalization and constant-folding batches. Phase J2 (exit tests T-NORM-01…14, T-FOLD-01…04).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §9 (rewrite/); cases from PHASEJ_QIR_CHECKLIST.md
//! (Normalization/elision/folding matrix).
//!
//! T-NORM-01 map fusion
//! T-NORM-02 filter through map
//! T-NORM-03 flat-map assoc
//! T-NORM-04 identity elim
//! T-NORM-05 nested comprehension collapse
//! T-NORM-06 dead-field elim
//! T-NORM-07 TopN fusion
//! T-NORM-08 double-order-by collapse
//! T-NORM-09 filter conjunction split
//! T-NORM-10 limit 0 elim
//! T-NORM-11 redundant distinct elim under keys
//! T-NORM-12 idempotence of the full batch (fixpoint stable)
//! T-NORM-13 per-rule dump snapshot
//! T-NORM-14 disable-flag honored
//!
//! T-FOLD-01 pure folded
//! T-FOLD-02 stable folded within query boundary only
//! T-FOLD-03 volatile never folded
//! T-FOLD-04 folding respects Option semantics
