/*! Goal representation for the trait solver. */

use yelang_ty::predicate::{ParamEnv, Predicate};

/// A goal to prove.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Goal<'tcx> {
    pub param_env: ParamEnv<'tcx>,
    pub predicate: Predicate<'tcx>,
}

impl<'tcx> Goal<'tcx> {
    pub fn new(param_env: ParamEnv<'tcx>, predicate: Predicate<'tcx>) -> Self {
        Self { param_env, predicate }
    }
}
