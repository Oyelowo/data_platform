//! Ordering/range end-to-end incl. TopN fusion and pushdown. Phase J5 (exit tests T-ORD-01…10).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §4.3–§4.4; cases from PHASEJ_QIR_CHECKLIST.md (Ordering matrix).
//!
//! T-ORD-01 range on Coll type error
//! T-ORD-02 range on Seq ok
//! T-ORD-03 TopN pushdown post decorrelation
//! T-ORD-04 ordering reuse (no double sort)
//! T-ORD-05 order honesty under `--shuffle-coll-batches` (whole suite)
//! T-ORD-06 desc/asc mixed keys
//! T-ORD-07 range with open ends
//! T-ORD-08 range on group members
//! T-ORD-09 order-by key referencing computed col
//! T-ORD-10 limit+offset semantics exact
