//! Tests for the token introspection API.

use std::ops::ControlFlow;

use yelang_proc_macro::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
    introspect::{TokenWalker, count_tokens, find_idents, walk_stream, walk_tree},
};

fn ident(name: &str) -> TokenTree {
    TokenTree::Ident(Ident::new(name, Span::call_site()))
}

fn punct(ch: char, spacing: Spacing) -> TokenTree {
    TokenTree::Punct(Punct::new(ch, spacing, Span::call_site()))
}

fn lit(text: &str) -> TokenTree {
    TokenTree::Literal(Literal::integer(text, Span::call_site()))
}

fn stream(trees: impl IntoIterator<Item = TokenTree>) -> TokenStream {
    trees.into_iter().collect()
}

// ---------------------------------------------------------------------------
// TokenWalker
// ---------------------------------------------------------------------------

struct CollectIdents(Vec<String>);

impl TokenWalker for CollectIdents {
    fn visit_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
        self.0.push(ident.value().to_string());
        ControlFlow::Continue(())
    }
}

#[test]
fn walker_collects_idents_at_top_level() {
    let s = stream([ident("a"), punct('+', Spacing::Alone), ident("b")]);
    let mut collector = CollectIdents(Vec::new());
    assert!(walk_stream(&s, &mut collector).is_continue());
    assert_eq!(collector.0, vec!["a", "b"]);
}

#[test]
fn walker_recurse_into_groups() {
    let inner = stream([ident("x"), punct('+', Spacing::Alone), ident("y")]);
    let group = Group::new(Delimiter::Parenthesis, inner, Span::call_site());
    let s = stream([ident("a"), TokenTree::Group(group), ident("b")]);

    let mut collector = CollectIdents(Vec::new());
    assert!(walk_stream(&s, &mut collector).is_continue());
    assert_eq!(collector.0, vec!["a", "x", "y", "b"]);
}

#[test]
fn walker_visits_enter_and_leave_group() {
    struct GroupRecorder(Vec<&'static str>);

    impl TokenWalker for GroupRecorder {
        fn visit_enter_group(&mut self, _group: &Group) -> ControlFlow<()> {
            self.0.push("enter");
            ControlFlow::Continue(())
        }

        fn visit_leave_group(&mut self, _group: &Group) -> ControlFlow<()> {
            self.0.push("leave");
            ControlFlow::Continue(())
        }
    }

    let inner = stream([ident("x")]);
    let group = Group::new(Delimiter::Brace, inner, Span::call_site());
    let s = stream([TokenTree::Group(group)]);

    let mut recorder = GroupRecorder(Vec::new());
    assert!(walk_stream(&s, &mut recorder).is_continue());
    assert_eq!(recorder.0, vec!["enter", "leave"]);
}

#[test]
fn walker_can_short_circuit() {
    struct StopAtB(bool);

    impl TokenWalker for StopAtB {
        fn visit_ident(&mut self, ident: &Ident) -> ControlFlow<()> {
            if ident.value() == "b" {
                self.0 = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }
    }

    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut stopper = StopAtB(false);
    let result = walk_stream(&s, &mut stopper);
    assert!(result.is_break());
    assert!(stopper.0);
}

#[test]
fn walker_default_forwards_to_visit_token() {
    struct CountTokens(usize);

    impl TokenWalker for CountTokens {
        fn visit_token(&mut self, _tree: &TokenTree) -> ControlFlow<()> {
            self.0 += 1;
            ControlFlow::Continue(())
        }
    }

    let s = stream([ident("a"), lit("1"), punct('+', Spacing::Alone)]);
    let mut counter = CountTokens(0);
    assert!(walk_stream(&s, &mut counter).is_continue());
    assert_eq!(counter.0, 3);
}

#[test]
fn walk_tree_visits_single_tree() {
    let mut collector = CollectIdents(Vec::new());
    assert!(walk_tree(&ident("foo"), &mut collector).is_continue());
    assert_eq!(collector.0, vec!["foo"]);
}

// ---------------------------------------------------------------------------
// Convenience helpers
// ---------------------------------------------------------------------------

#[test]
fn count_tokens_counts_recursively() {
    let inner = stream([ident("x"), ident("y")]);
    let group = Group::new(Delimiter::Parenthesis, inner, Span::call_site());
    let s = stream([ident("a"), TokenTree::Group(group), ident("b")]);

    assert_eq!(count_tokens(&s), 5);
}

#[test]
fn count_tokens_empty_stream() {
    let s = stream([]);
    assert_eq!(count_tokens(&s), 0);
}

#[test]
fn find_idents_collects_recursively() {
    let inner = stream([ident("inner")]);
    let group = Group::new(Delimiter::Brace, inner, Span::call_site());
    let s = stream([ident("outer"), TokenTree::Group(group)]);

    assert_eq!(find_idents(&s), vec!["outer", "inner"]);
}
