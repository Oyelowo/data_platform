mod optimizer;
pub mod decorrelate;
pub mod pushdown;
pub mod simplify;
pub mod prune;

pub use optimizer::{OptRule, ApplyOrder, Optimizer, default_rules};
