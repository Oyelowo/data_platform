//! Newtyped indices for QIR operators, physical operators, exec operators,
//! scalar expressions, and binders.

use std::fmt;

use yelang_arena::index_vec::{Idx, IndexVec};

macro_rules! define_id {
    ($name:ident) => {
        /// Newtyped identifier.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            /// Return the raw integer value.
            pub fn raw(self) -> u32 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl Idx for $name {
            fn from_usize(idx: usize) -> Self {
                $name(idx as u32)
            }

            fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

define_id!(LirId);
define_id!(PirId);
define_id!(ExecId);
define_id!(QExprId);
define_id!(BinderId);

/// Arena for logical operators.
pub type LirArena<T> = IndexVec<LirId, T>;
/// Arena for physical operators.
pub type PirArena<T> = IndexVec<PirId, T>;
/// Arena for exec operators.
pub type ExecArena<T> = IndexVec<ExecId, T>;
/// Arena for scalar expressions.
pub type QExprArena<T> = IndexVec<QExprId, T>;
/// Arena for binder metadata.
pub type BinderArena<T> = IndexVec<BinderId, T>;

/// Trait for IDs that can be used as arena keys.
pub trait ArenaId: Idx + Copy + Eq + fmt::Debug {}
impl ArenaId for LirId {}
impl ArenaId for PirId {}
impl ArenaId for ExecId {}
impl ArenaId for QExprId {}
impl ArenaId for BinderId {}
