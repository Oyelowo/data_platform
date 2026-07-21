//! Per-group TopN rewrite (partition = correlation cols) plus ROW_NUMBER fallback; OFFSET carried through both.
//! Phase J3 (J3.7). Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §4.2, §4.3.
