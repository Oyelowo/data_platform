use std::fmt;

use yelang_interner::Interner;

use super::{TokenStream, TokenTree};

/// Write the rendered source text of a token stream to `f`.
pub fn write_token_stream(
    stream: &TokenStream,
    f: &mut dyn fmt::Write,
    interner: &Interner,
) -> fmt::Result {
    write!(f, "{}", stream.render(interner))
}

/// Write the rendered source text of a single token tree to `f`.
pub fn write_token_tree(
    tree: &TokenTree,
    f: &mut dyn fmt::Write,
    interner: &Interner,
) -> fmt::Result {
    write!(f, "{}", tree.render(interner))
}
