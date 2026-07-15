use std::fmt;

pub(crate) use yelang_interner::{Interner, Symbol};

pub trait Codegen {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result;
}

impl<T: Codegen + ?Sized> Codegen for Box<T> {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result {
        (**self).codegen(f, interner)
    }
}

pub mod codegen;
mod common;
pub mod expr;
pub mod item;
pub mod parse;
mod pattern;
mod program;
pub mod ptr;
pub mod query;
mod stmt;
pub mod token;
pub mod tokenizer;
mod types;
pub mod validation;
pub mod visit;

#[cfg(test)]
mod test;

pub use codegen::*;
pub use common::*;
pub use expr::*;
pub use item::*;
pub use parse::*;
pub use pattern::*;
pub use program::*;
pub use ptr::*;
pub use query::*;
pub use stmt::*;
pub use tokenizer::*;
pub use types::*;
pub use visit::*;
