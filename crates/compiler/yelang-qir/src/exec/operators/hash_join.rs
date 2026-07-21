//! Hash join: bloom + min/max dynamic filter pushdown, skew handling via morsels, spill; DiamondJoin lookup/expand-ready.
//! Phase J7 (J7.4). Design: PHASEJ_QIR_STRUCTURE_DESIGN.md §7, §8.3–8.4.
