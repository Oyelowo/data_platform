/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

//! Parser-token surface re-exported from `yelang-lexer`.

pub mod tokens_macros;

pub use tokens_macros::*;
pub use yelang_lexer::tokenizer as tokens;
pub use yelang_lexer::tokenizer::*;
