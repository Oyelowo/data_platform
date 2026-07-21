//! THIR identifier types.
//!
//! Expressions, patterns, statements, and bodies each live in their own ID
//! space. `ThirBodyId` is the unit of THIR construction: a complete function,
//! closure, or inline branch body.

use yelang_arena::new_key_type;

new_key_type! {
    pub struct ThirExprId;
    pub struct ThirPatId;
    pub struct ThirStmtId;
    pub struct ThirBodyId;
}
