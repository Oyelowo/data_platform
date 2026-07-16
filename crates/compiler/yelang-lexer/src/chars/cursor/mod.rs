/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

mod consume;
mod core;
mod parse;
mod types;

pub use core::CharCursor;
pub use types::{Checkpoint, FileId, Position, ROOT_SYNTAX_CONTEXT, Span};
