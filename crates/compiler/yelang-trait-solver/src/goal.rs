/*! Goal representation for the trait solver. */

use yelang_ty::fold::TypeFoldable;
use yelang_ty::predicate::{ParamEnv, Predicate};

/// A goal to prove.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Goal {
    pub param_env: ParamEnv,
    pub predicate: Predicate,
}

impl Goal {
    pub fn new(param_env: ParamEnv, predicate: Predicate) -> Self {
        Self {
            param_env,
            predicate,
        }
    }
}

impl TypeFoldable for Goal {
    fn fold_with<F: yelang_ty::fold::TypeFolder>(self, folder: &mut F) -> Self {
        Goal {
            param_env: ParamEnv {
                caller_bounds: self.param_env.caller_bounds.fold_with(folder),
            },
            predicate: self.predicate.fold_with(folder),
        }
    }
}
