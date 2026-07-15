use std::fmt;

use crate::{Codegen, Interner};

impl Codegen for yelang_macro_core::token_tree::TokenStream {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", self.render(interner))
    }
}

impl Codegen for yelang_macro_core::token_tree::TokenTree {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", self.render(interner))
    }
}
