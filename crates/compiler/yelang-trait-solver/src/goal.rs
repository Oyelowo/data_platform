/*! Goal representation for the trait solver. */

use yelang_ty::fold::TypeFoldable;
use yelang_ty::predicate::{ParamEnv, Predicate};

/// A goal to prove.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Goal<'tcx> {
    pub param_env: ParamEnv<'tcx>,
    pub predicate: Predicate<'tcx>,
}

impl<'tcx> Goal<'tcx> {
    pub fn new(param_env: ParamEnv<'tcx>, predicate: Predicate<'tcx>) -> Self {
        Self {
            param_env,
            predicate,
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Goal<'tcx> {
    fn fold_with<F: yelang_ty::fold::TypeFolder<'tcx>>(self, folder: &mut F) -> Self {
        Goal {
            param_env: ParamEnv {
                caller_bounds: self.param_env.caller_bounds.fold_with(folder),
            },
            predicate: self.predicate.fold_with(folder),
        }
    }
}
