/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 *
 * AST validation scaffolding.
 *
 * This module is intentionally lightweight today:
 * - Parsing remains mostly strict (TokenResult-based).
 * - Future work may add error-recovery and embed errors in AST nodes.
 * - When that happens, this module can host "semantic-ish" AST validations
 *   (shape invariants, structural constraints, binder rules) that should not
 *   live in the tokenizer/parser.
 */

mod link;
mod update;

mod error;
mod validate;

pub use link::*;
pub use update::validate_update_stmt;

pub use error::*;
pub use validate::*;
