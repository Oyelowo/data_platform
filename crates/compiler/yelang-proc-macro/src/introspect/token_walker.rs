//! High-level token-tree walker.

use std::ops::ControlFlow;

use crate::{Group, Ident, Literal, Punct, TokenStream, TokenTree};

/// Visitor trait for token-tree traversal.
///
/// Implementations can override any of the `visit_*` methods to inspect
/// tokens. The default implementations forward to
/// [`visit_token`](Self::visit_token), so a walker that only cares about
/// every token tree can override just that one method.
///
/// Groups are visited twice: first via [`visit_enter_group`](Self::visit_enter_group)
/// before recursing into their contents, then via [`visit_leave_group`](Self::visit_leave_group)
/// after the recursion completes. The group itself is also passed to
/// [`visit_group`](Self::visit_group) and [`visit_token`](Self::visit_token)
/// at the enter point.
///
/// Methods return [`ControlFlow<()>`] so callers can short-circuit traversal
/// by returning [`ControlFlow::Break(())`].
pub trait TokenWalker {
    /// Called for every token tree in the stream, including groups.
    fn visit_token(&mut self, _tree: &TokenTree) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    /// Called when an identifier is encountered.
    fn visit_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
        self.visit_token(&TokenTree::Ident(ident.clone()))
    }

    /// Called when a punctuation token is encountered.
    fn visit_punct(&mut self, punct: &Punct) -> ControlFlow<()> {
        self.visit_token(&TokenTree::Punct(punct.clone()))
    }

    /// Called when a literal token is encountered.
    fn visit_literal(&mut self, literal: &Literal) -> ControlFlow<()> {
        self.visit_token(&TokenTree::Literal(literal.clone()))
    }

    /// Called when a group token is encountered.
    fn visit_group(&mut self, group: &Group) -> ControlFlow<()> {
        self.visit_token(&TokenTree::Group(group.clone()))
    }

    /// Called before recursing into a group's inner stream.
    fn visit_enter_group(&mut self, group: &Group) -> ControlFlow<()> {
        self.visit_group(group)
    }

    /// Called after recursing into a group's inner stream.
    fn visit_leave_group(&mut self, _group: &Group) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

/// Walk `stream` and dispatch to `visitor`.
///
/// Returns [`ControlFlow::Break(())`] as soon as any callback returns it.
pub fn walk_stream(stream: &TokenStream, visitor: &mut impl TokenWalker) -> ControlFlow<()> {
    for tree in stream.iter() {
        walk_tree(&tree, visitor)?;
    }
    ControlFlow::Continue(())
}

/// Walk a single token tree and dispatch to `visitor`.
///
/// Returns [`ControlFlow::Break(())`] as soon as any callback returns it.
pub fn walk_tree(tree: &TokenTree, visitor: &mut impl TokenWalker) -> ControlFlow<()> {
    match tree {
        TokenTree::Ident(ident) => visitor.visit_ident(ident)?,
        TokenTree::Punct(punct) => visitor.visit_punct(punct)?,
        TokenTree::Literal(literal) => visitor.visit_literal(literal)?,
        TokenTree::Group(group) => {
            visitor.visit_enter_group(group)?;
            walk_stream(&group.stream(), visitor)?;
            visitor.visit_leave_group(group)?;
        }
    }
    ControlFlow::Continue(())
}

/// Count every token tree in `stream`, recursively including group contents.
pub fn count_tokens(stream: &TokenStream) -> usize {
    struct Counter(usize);

    impl TokenWalker for Counter {
        fn visit_token(&mut self, _tree: &TokenTree) -> ControlFlow<()> {
            self.0 += 1;
            ControlFlow::Continue(())
        }
    }

    let mut counter = Counter(0);
    let _ = walk_stream(stream, &mut counter);
    counter.0
}

/// Collect every identifier value in `stream`, recursively including group contents.
pub fn find_idents(stream: &TokenStream) -> Vec<String> {
    struct IdentCollector(Vec<String>);

    impl TokenWalker for IdentCollector {
        fn visit_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
            self.0.push(ident.value().to_string());
            ControlFlow::Continue(())
        }
    }

    let mut collector = IdentCollector(Vec::new());
    let _ = walk_stream(stream, &mut collector);
    collector.0
}
