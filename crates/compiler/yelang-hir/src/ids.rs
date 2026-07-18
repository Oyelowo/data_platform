//! HIR identifier types.
//!
//! Items are keyed by dense `DefId`s from name resolution.  HIR nodes
//! (expressions, patterns, statements, types, and bodies) are stored in
//! slotmap arenas and referenced by the typed keys defined here.

use yelang_arena::new_key_type;

pub use yelang_arena::DefId;

new_key_type! {
    pub struct ExprId;
    pub struct PatId;
    pub struct StmtId;
    pub struct HirTyId;
    pub struct BodyId;
    pub struct QueryId;
}

/// Items are keyed by their `DefId` from name resolution.
pub type ItemId = DefId;
