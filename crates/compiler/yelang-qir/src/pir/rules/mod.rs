//! Physical implementation rules: LIR operator -> PhysOp candidates, plus
//! property enforcers. Phase J6 (J6.3).
//! Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §6, §9.

pub mod aggregate;
pub mod exchange;
pub mod joins;
pub mod scans;
pub mod sort;
pub mod window;
