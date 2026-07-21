//! Scalar IR (QExpr) shared by the logical (LIR) and physical (PIR) layers.
//! Phase J1 (J1.1). Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §3.1, §9.

pub mod binder;
pub mod fold;
pub mod print;
pub mod qexpr;
pub mod visit;

pub use qexpr::*;
