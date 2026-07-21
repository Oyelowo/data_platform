//! Dead source/binding elision. Phase J2 (exit tests T-ELIDE-01…06).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §9 (rewrite/); cases from PHASEJ_QIR_CHECKLIST.md
//! (Normalization/elision/folding matrix).
//!
//! T-ELIDE-01 dead source elided
//! T-ELIDE-02 dead link elided
//! T-ELIDE-03 `select 1` zero reads (counting scan)
//! T-ELIDE-04 demanded field subset reaches scan
//! T-ELIDE-05 group-by dead key elim
//! T-ELIDE-06 elision never changes type of result
