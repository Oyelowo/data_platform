//! Projection/root-label scope, binder contexts. Phase J0 (J0.10; exit tests T-SCOPE-01…08).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §2.5; cases from PHASEJ_QIR_CHECKLIST.md (Scoping matrix).
//!
//! T-SCOPE-01 `select users[*] from users@u` ok
//! T-SCOPE-02 `select u from users@u` error + suggestion
//! T-SCOPE-03 `where u.age > 1` ok
//! T-SCOPE-04 `order by u.age` ok
//! T-SCOPE-05 `group by { c: u.city } into g` ok
//! T-SCOPE-06 `select g[*].{…} … into g` ok
//! T-SCOPE-07 links binders usable in link modifiers, not bare in projection
//! T-SCOPE-08 nested selector re-binds `u` (`users@u[*].{…}`) independently of from-binder
