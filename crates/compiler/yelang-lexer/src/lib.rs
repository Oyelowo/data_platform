/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 27/01/2025
 */

pub(crate) use yelang_interner::{Interner, Symbol};

pub mod bytes;
pub mod chars;
pub mod data;
pub mod helper_types;
pub mod tokenizer;
pub mod tokens;

mod macros;

pub use bytes::*;
pub use chars::*;
pub use data::*;
pub use helper_types::*;
pub use tokenizer::*;
pub use tokens::*;

// Internal compatibility shim: many internal modules still reference `crate::...`.
// This is intentionally NOT part of the public API surface.
