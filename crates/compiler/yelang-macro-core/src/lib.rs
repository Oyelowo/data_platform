/*!
 * Yelang Macro Core
 *
 * Shared token tree, hygiene, and macro ID types used by the parser, macro
 * expander, and future declarative macro implementation.
 */

pub mod hygiene;
pub mod id;
pub mod token_tree;

pub use hygiene::HygieneData;
pub use id::{
    CrateId, ExpnArena, ExpnData, ExpnId, ExpnKind, MacroDefArena, MacroDefData, MacroDefId,
    MacroKind, SyntaxContextArena, SyntaxContextData, SyntaxContextId, TagToken, TokenId,
    Transparency,
};
pub use token_tree::*;
