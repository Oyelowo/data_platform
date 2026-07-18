/*! Snapshot management for speculative inference.
 *
 * A `Snapshot` records the state of all four unification tables so that a
 * probe can roll them back atomically.
 */

use crate::unify::Snapshot as TableSnapshot;

/// A snapshot of the entire inference context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub(crate) ty: TableSnapshot,
    pub(crate) int: TableSnapshot,
    pub(crate) float: TableSnapshot,
    pub(crate) const_: TableSnapshot,
}
