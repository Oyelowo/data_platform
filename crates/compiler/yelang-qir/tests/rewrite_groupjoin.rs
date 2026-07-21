//! GroupJoin eager/lazy legality and rewrite. Phase J4 (exit tests T-GJ-01…08).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §5.3; cases from PHASEJ_QIR_CHECKLIST.md (GroupJoin matrix).
//!
//! T-GJ-01 eager legal: PK/FK + key match -> final group-by elided
//! T-GJ-02 eager illegal: non-superkey -> lazy
//! T-GJ-03 count(*) multiplier correction exact on duplicated right side
//! T-GJ-04 empty group neutral elements
//! T-GJ-05 cost choice picks lazy on selective right
//! T-GJ-06 parallel merge correct
//! T-GJ-07 HAVING-as-filter on group records
//! T-GJ-08 differential vs reference on random PK/FK data
