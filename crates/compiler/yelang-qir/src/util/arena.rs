//! Arena helpers for QIR.

use yelang_arena::index_vec::IndexVec;

/// Ensure an `IndexVec` has a slot for the given key, growing with default
/// values if necessary.
pub fn ensure_slot<K, V>(vec: &mut IndexVec<K, V>, key: K) -> &mut V
where
    K: yelang_arena::index_vec::Idx,
    V: Default,
{
    vec.resize_for_key(key)
}
