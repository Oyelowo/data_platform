//! Kernel recognition, sugar inlining, override respect. Phase J0 (exit tests T-REC-01…12).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §2.1–§2.4; cases from PHASEJ_QIR_CHECKLIST.md (Recognition matrix).
//!
//! T-REC-01 kernel lang item on trait method recognized via resolution
//! T-REC-02 impl-item resolution recognized (not just trait-item)
//! T-REC-03 sugar method inlines default body -> Aggregate operator
//! T-REC-04 user-overridden sugar method honored (no inlining of default)
//! T-REC-05 user fn named `sum` NOT mistaken for aggregate (shadowing)
//! T-REC-06 aggregate over element (`sum(u)`) -> type error + suggestion
//! T-REC-07 free-function `sum(users)` form (if stdlib provides it)
//! T-REC-08 `.query()` adapter on `[T]` lowers identically to comprehension form
//! T-REC-09 no `Queryable` impl for `[T]` (trait-bound error where expected)
//! T-REC-10 custom UDA: zero compiler changes, full pipeline
//! T-REC-11 `@lang` on trait items collected by resolver
//! T-REC-12 `QueryableMethod` keyed by lang item only (no name table remains — grep test)
