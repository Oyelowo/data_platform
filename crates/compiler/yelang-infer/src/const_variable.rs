/*! Const inference variable support. */

use yelang_ty::ty::{Const, ConstVid};

use crate::unify::UnificationTable;

/// Value stored for a const inference variable.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstVarValue<'tcx> {
    Known(Const<'tcx>),
    Unknown,
}

impl<'tcx> Default for ConstVarValue<'tcx> {
    fn default() -> Self {
        ConstVarValue::Unknown
    }
}

/// Table of const inference variables.
pub type ConstVariableTable<'tcx> = UnificationTable<ConstVid, ConstVarValue<'tcx>>;
