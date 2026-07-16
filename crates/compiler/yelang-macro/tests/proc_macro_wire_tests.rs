//! Tests for compiler-internal token stream ↔ wire token stream conversion.

use yelang_interner::Interner;
use yelang_macro::proc_macro::{core_to_wire, wire_to_core};
use yelang_macro_core::{Delimiter, Group, Ident, Literal, Punct, Spacing, TokenStream, TokenTree};

fn ident(sym: yelang_interner::Symbol) -> TokenTree {
    TokenTree::Ident(Ident::new(sym, yelang_macro_core::Span::default()))
}

fn punct(ch: char, spacing: Spacing) -> TokenTree {
    TokenTree::Punct(Punct::new(ch, spacing, yelang_macro_core::Span::default()))
}

fn lit_int(sym: yelang_interner::Symbol) -> TokenTree {
    TokenTree::Literal(Literal::int(sym, yelang_macro_core::Span::default()))
}

fn group(delimiter: Delimiter, trees: Vec<TokenTree>) -> TokenTree {
    TokenTree::Group(Group::new(
        delimiter,
        TokenStream::from_vec(trees),
        yelang_macro_core::Span::default(),
    ))
}

#[test]
fn core_to_wire_resolves_identifier_text() {
    let interner = Interner::new();
    let sym = interner.get_or_intern("hello");
    let stream = TokenStream::from_vec(vec![ident(sym)]);

    let wire = core_to_wire(&stream, &interner);
    assert_eq!(wire.trees.len(), 1);
    match &wire.trees[0] {
        yelang_proc_macro_bridge::protocol::token::WireTokenTree::Ident {
            text, is_raw, ..
        } => {
            assert_eq!(text, "hello");
            assert!(!is_raw);
        }
        other => panic!("expected ident, got {:?}", other),
    }
}

#[test]
fn core_to_wire_preserves_delimiters() {
    let interner = Interner::new();
    let x = interner.get_or_intern("x");
    let one = interner.get_or_intern("1");
    let stream = TokenStream::from_vec(vec![group(
        Delimiter::Brace,
        vec![ident(x), punct('+', Spacing::Alone), lit_int(one)],
    )]);

    let wire = core_to_wire(&stream, &interner);
    match &wire.trees[0] {
        yelang_proc_macro_bridge::protocol::token::WireTokenTree::Group {
            delimiter,
            trees,
            ..
        } => {
            assert_eq!(
                *delimiter,
                yelang_proc_macro_bridge::protocol::token::WireDelimiter::Brace
            );
            assert_eq!(trees.len(), 3);
        }
        other => panic!("expected group, got {:?}", other),
    }
}

#[test]
fn core_to_wire_preserves_punctuation_spacing() {
    let interner = Interner::new();
    let stream =
        TokenStream::from_vec(vec![punct('<', Spacing::Joint), punct('=', Spacing::Alone)]);

    let wire = core_to_wire(&stream, &interner);
    match (&wire.trees[0], &wire.trees[1]) {
        (
            yelang_proc_macro_bridge::protocol::token::WireTokenTree::Punct {
                spacing: yelang_proc_macro_bridge::protocol::token::WireSpacing::Joint,
                ..
            },
            yelang_proc_macro_bridge::protocol::token::WireTokenTree::Punct {
                spacing: yelang_proc_macro_bridge::protocol::token::WireSpacing::Alone,
                ..
            },
        ) => {}
        other => panic!("expected joint/alone spacing, got {:?}", other),
    }
}

#[test]
fn wire_to_core_round_trips_identifiers() {
    let interner = Interner::new();
    let foo = interner.get_or_intern("foo");
    let bar = interner.get_or_intern("bar");
    let stream = TokenStream::from_vec(vec![ident(foo), ident(bar)]);
    let wire = core_to_wire(&stream, &interner);

    let core = wire_to_core(wire, &interner, yelang_lexer::Span::default()).unwrap();
    let trees: Vec<_> = core.iter().collect();
    assert_eq!(trees.len(), 2);
    match &trees[0] {
        TokenTree::Ident(i) => {
            assert_eq!(interner.resolve(&i.sym), "foo");
        }
        other => panic!("expected ident, got {:?}", other),
    }
}

#[test]
fn wire_to_core_round_trips_groups() {
    let interner = Interner::new();
    let x = interner.get_or_intern("x");
    let stream = TokenStream::from_vec(vec![group(Delimiter::Parenthesis, vec![ident(x)])]);
    let wire = core_to_wire(&stream, &interner);

    let core = wire_to_core(wire, &interner, yelang_lexer::Span::default()).unwrap();
    match &core.iter().collect::<Vec<_>>()[0] {
        TokenTree::Group(g) => {
            assert_eq!(g.delimiter, Delimiter::Parenthesis);
            assert_eq!(g.stream.len(), 1);
        }
        other => panic!("expected group, got {:?}", other),
    }
}

#[test]
fn wire_to_core_preserves_compound_operators() {
    let interner = Interner::new();
    let stream = TokenStream::from_vec(vec![
        ident(interner.get_or_intern("x")),
        punct('<', Spacing::Joint),
        punct('=', Spacing::Alone),
        ident(interner.get_or_intern("y")),
    ]);
    let wire = core_to_wire(&stream, &interner);

    let core = wire_to_core(wire, &interner, yelang_lexer::Span::default()).unwrap();
    let rendered = core.render(&interner);
    assert_eq!(rendered, "x<=y");
}
