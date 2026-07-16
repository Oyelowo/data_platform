//! High-level token-tree walker.

use crate::{TokenStream, TokenTree};

/// Walks a token stream and invokes a callback for each token tree.
pub struct TokenWalker;

impl TokenWalker {
    pub fn walk<F: FnMut(&TokenTree)>(stream: &TokenStream, mut f: F) {
        for tree in stream.iter() {
            f(&tree);
            if let TokenTree::Group(g) = &tree {
                Self::walk(&g.stream(), &mut f);
            }
        }
    }
}
