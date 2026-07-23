//! MIR optimization passes.
//!
//! Passes run in a fixed order on each MIR body:
//! 1. SimplifyCfg — merge redundant blocks, remove unreachable blocks
//! 2. DCE — remove unused locals and dead assignments
//! 3. ConstFold — fold constant expressions
//! 4. Inline — inline small function calls (future)

pub mod dce;
pub mod simplify;

use crate::body::Body;

/// A MIR optimization pass.
pub trait MirPass {
    /// Human-readable name.
    fn name(&self) -> &str;

    /// Run the pass on a MIR body, modifying it in place.
    fn run(&self, body: &mut Body);
}

/// Run all MIR optimization passes on a body.
pub fn run_passes(body: &mut Body) {
    let passes: Vec<Box<dyn MirPass>> = vec![
        Box::new(simplify::SimplifyCfg),
        Box::new(dce::DeadCodeElimination),
        Box::new(simplify::ConstFold),
    ];

    for pass in &passes {
        pass.run(body);
    }
}
