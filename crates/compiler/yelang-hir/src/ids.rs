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
    pub struct TyId;
    pub struct BodyId;
    pub struct ItemKindId;
    pub struct TraitItemKindId;
    pub struct ImplItemKindId;
    pub struct ForeignItemKindId;
}
