//! Query optimization.
//!
//! Two phases:
//! 1. **Logical optimization** ([`crate::logical::optimize`]): Plan → Plan
//!    transformations that preserve query semantics (decorrelation, pushdown,
//!    simplify, join reordering, projection pruning).
//! 2. **Physical planning** ([`crate::physical`]): Plan → PhysPlan
//!    transformations that choose execution algorithms and insert Exchange nodes.
//!
//! This module holds the optimizer driver itself — the [`OptRule`] trait and
//! the fixpoint [`Optimizer`] that applies the logical passes.

mod optimizer;

pub use optimizer::{default_rules, ApplyOrder, OptRule, Optimizer};
