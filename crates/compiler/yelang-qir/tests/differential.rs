//! Property-based reference-vs-optimized differential testing, random schemas. Phase J7 (T-DIFF-01…06).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §8.1; cases from PHASEJ_QIR_CHECKLIST.md (Differential matrix).
//!
//! T-DIFF-01 random schemas (1–6 sources, PK/FK, Option columns) x random queries from the full
//!         grammar x reference-vs-optimized — 100k cases
//! T-DIFF-02 bag-equality for Coll, seq-equality for Seq
//! T-DIFF-03 same for grouped+aggregated queries
//! T-DIFF-04 same under forced spill
//! T-DIFF-05 same under shuffled Coll batches
//! T-DIFF-06 same at parallelism 1 and 8
