//! Bottom-up logical analyses over LIR plans feeding rewrites and decorrelation.
//! Phase J1–J3. Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §3.1 (props), §4.1.

pub mod correlation;
pub mod demand;
pub mod keys;
