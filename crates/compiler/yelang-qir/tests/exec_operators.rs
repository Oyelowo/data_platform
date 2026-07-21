//! Per-operator unit tests incl. spill paths. Phase J7 (exit tests T-EXEC-01…20).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §7; cases from PHASEJ_QIR_CHECKLIST.md (Exec matrix).
//!
//! T-EXEC-01 batch validity bitmaps
//! T-EXEC-02 kernel per type/op (exhaustive numeric combinations, overflow policy)
//! T-EXEC-03 filter/project fusion == unfused
//! T-EXEC-04 hash join correct on skew
//! T-EXEC-05 dynamic filter pushed (instrumented)
//! T-EXEC-06 hash agg partial/final
//! T-EXEC-07 spill join/agg/sort under 1 MB pool
//! T-EXEC-08 sort stability + TopN partition correctness
//! T-EXEC-09 morsel work-stealing terminates under straggler injection
//! T-EXEC-10 memory arbitrator forces spill, no OOM, results identical
//! T-EXEC-11 exchange local credit: no unbounded buffer
//! T-EXEC-12 group_join operator
//! T-EXEC-13 merge_join on sorted inputs
//! T-EXEC-14 distinct hash vs sort
//! T-EXEC-15 limit short-circuit (early termination counted)
//! T-EXEC-16 traversal expansion
//! T-EXEC-17 Option through every operator
//! T-EXEC-18 result arena freed (leak check)
//! T-EXEC-19 empty-input every operator
//! T-EXEC-20 single-row every operator
