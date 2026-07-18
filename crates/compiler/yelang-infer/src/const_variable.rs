/*! Const inference variable support. */

use yelang_ty::ty::{ConstId, ConstVid};

use crate::unify::UnificationTable;

/// Value stored for a const inference variable.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstVarValue {
    Known(ConstId),
    Unknown,
}

impl Default for ConstVarValue {
    fn default() -> Self {
        ConstVarValue::Unknown
    }
}

/// Table of const inference variables.
pub type ConstVariableTable = UnificationTable<ConstVid, ConstVarValue>;
