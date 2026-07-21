//! Rewrite batch: constant folding over pure regions; volatile calls are never folded.
//! Phase J2 (J2.4). Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §8 (volatility gating), §9.
