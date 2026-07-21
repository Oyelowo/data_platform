//! Impl-body extraction per built-in aggregate + a custom UDA. Phase J1 (exit tests T-AGG-01…10).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §2.3, §5.1; cases from PHASEJ_QIR_CHECKLIST.md (Aggregates matrix).
//!
//! T-AGG-01..06 each built-in (sum/product/count/avg/min/max) over i32/i64/f64
//! T-AGG-07 avg on empty input
//! T-AGG-08 min/max -> Option (empty -> None)
//! T-AGG-09 count over empty -> 0
//! T-AGG-10 custom UDA (e.g. StringConcat) end-to-end
