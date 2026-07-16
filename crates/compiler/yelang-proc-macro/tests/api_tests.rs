//! Tests for the public procedural macro API.

use yelang_proc_macro::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
};

#[test]
fn ident_stores_value() {
    let id = Ident::new("foo", Span::call_site());
    assert_eq!(id.value(), "foo");
}

#[test]
fn literal_integer_renders() {
    let lit = Literal::integer("42", Span::call_site());
    assert_eq!(lit.to_string(), "42");
}

#[test]
fn literal_string_renders() {
    let lit = Literal::string("hello", Span::call_site());
    assert_eq!(lit.to_string(), "\"hello\"");
}

#[test]
fn punct_stores_char_and_spacing() {
    let p = Punct::new('+', Spacing::Alone, Span::call_site());
    assert_eq!(p.as_char(), '+');
    assert_eq!(p.spacing(), Spacing::Alone);
}

#[test]
fn group_stream_round_trip() {
    let inner = TokenStream::from(TokenTree::Ident(Ident::new("x", Span::call_site())));
    let group = Group::new(Delimiter::Parenthesis, inner.clone(), Span::call_site());
    assert_eq!(group.delimiter(), Delimiter::Parenthesis);
    assert_eq!(group.stream(), inner);
}

#[test]
fn token_stream_from_iterator() {
    let stream: TokenStream = [
        TokenTree::Ident(Ident::new("a", Span::call_site())),
        TokenTree::Punct(Punct::new('+', Spacing::Alone, Span::call_site())),
        TokenTree::Ident(Ident::new("b", Span::call_site())),
    ]
    .into_iter()
    .collect();
    assert_eq!(stream.len(), 3);
    assert_eq!(stream.to_string(), "a + b");
}

#[test]
fn quote_emits_ident() {
    let stream = yelang_proc_macro::quote! { foo };
    assert_eq!(stream.to_string(), "foo");
}

#[test]
fn quote_emits_group() {
    let stream = yelang_proc_macro::quote! { ( x ) };
    assert_eq!(stream.to_string(), "(x)");
}

#[test]
fn quote_interpolates_ident() {
    let name = Ident::new("bar", Span::call_site());
    let stream = yelang_proc_macro::quote! { foo #name baz };
    assert_eq!(stream.to_string(), "foo bar baz");
}

#[test]
fn quote_interpolates_token_stream() {
    let inner = yelang_proc_macro::quote! { a + b };
    let stream = yelang_proc_macro::quote! { ( #inner ) };
    assert_eq!(stream.to_string(), "(a + b)");
}

#[test]
fn quote_repeats_without_separator() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
        Ident::new("c", Span::call_site()),
    ];
    let stream = yelang_proc_macro::quote! { #( #items )* };
    assert_eq!(stream.to_string(), "a b c");
}

#[test]
fn quote_repeats_with_comma_separator() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    let stream = yelang_proc_macro::quote! { #( #items ),* };
    assert_eq!(stream.to_string(), "a , b");
}

#[test]
fn quote_group_with_interpolation() {
    let name = Ident::new("x", Span::call_site());
    let stream = yelang_proc_macro::quote! { { #name } };
    assert_eq!(stream.to_string(), "{x}");
}

#[test]
fn quote_string_literal() {
    let stream = yelang_proc_macro::quote! { "hello" };
    assert_eq!(stream.to_string(), "\"hello\"");
}

#[test]
fn quote_integer_literal() {
    let stream = yelang_proc_macro::quote! { 42 };
    assert_eq!(stream.to_string(), "42");
}

#[test]
fn quote_interpolates_grouped_expression() {
    let value = Literal::integer("7", Span::call_site());
    let stream = yelang_proc_macro::quote! { 1 + #(value) };
    assert_eq!(stream.to_string(), "1 + 7");
}

#[test]
fn raw_string_literal_renders_with_hashes() {
    let lit = Literal::raw_string("hello", 1, Span::call_site());
    assert_eq!(lit.to_string(), "r#\"hello\"#");
}

#[test]
fn raw_string_literal_renders_without_hashes() {
    let lit = Literal::raw_string("hello", 0, Span::call_site());
    assert_eq!(lit.to_string(), "r\"hello\"");
}

#[test]
fn token_stream_render_source_preserves_compound_operators() {
    let stream: TokenStream = [
        TokenTree::Ident(Ident::new("x", Span::call_site())),
        TokenTree::Punct(Punct::new('<', Spacing::Joint, Span::call_site())),
        TokenTree::Punct(Punct::new('=', Spacing::Alone, Span::call_site())),
        TokenTree::Ident(Ident::new("y", Span::call_site())),
    ]
    .into_iter()
    .collect();
    let interner = yelang_interner::Interner::new();
    assert_eq!(stream.render_source(&interner), "x<=y");
}

#[test]
fn token_stream_render_source_separates_word_like_tokens() {
    let stream: TokenStream = [
        TokenTree::Ident(Ident::new("foo", Span::call_site())),
        TokenTree::Ident(Ident::new("bar", Span::call_site())),
    ]
    .into_iter()
    .collect();
    let interner = yelang_interner::Interner::new();
    assert_eq!(stream.render_source(&interner), "foo bar");
}

#[test]
fn from_core_stream_resolves_symbols() {
    let interner = yelang_interner::Interner::new();
    let sym = interner.get_or_intern("my_var");
    let span = yelang_macro_core::Span::default();
    let core_stream =
        yelang_macro_core::TokenStream::from_vec(vec![yelang_macro_core::TokenTree::Ident(
            yelang_macro_core::Ident::new(sym, span),
        )]);
    let proc_stream = TokenStream::from_core_stream(&core_stream, &interner);
    assert_eq!(proc_stream.to_string(), "my_var");
}

#[test]
fn from_core_stream_preserves_string_literals() {
    let interner = yelang_interner::Interner::new();
    let value = interner.get_or_intern("hello world");
    let span = yelang_macro_core::Span::default();
    let core_stream =
        yelang_macro_core::TokenStream::from_vec(vec![yelang_macro_core::TokenTree::Literal(
            yelang_macro_core::Literal::string(value, span),
        )]);
    let proc_stream = TokenStream::from_core_stream(&core_stream, &interner);
    assert_eq!(proc_stream.to_string(), "\"hello world\"");
}

#[test]
fn round_trip_core_stream_through_proc_macro() {
    let interner = yelang_interner::Interner::new();
    let sym = interner.get_or_intern("x");
    let span = yelang_macro_core::Span::default();
    let core_stream = yelang_macro_core::TokenStream::from_vec(vec![
        yelang_macro_core::TokenTree::Ident(yelang_macro_core::Ident::new(sym, span)),
        yelang_macro_core::TokenTree::Punct(yelang_macro_core::Punct::new(
            '+',
            yelang_macro_core::Spacing::Alone,
            span,
        )),
        yelang_macro_core::TokenTree::Literal(yelang_macro_core::Literal::int(
            interner.get_or_intern("1"),
            span,
        )),
    ]);
    let proc_stream = TokenStream::from_core_stream(&core_stream, &interner);
    let rendered = proc_stream.render_source(&interner);
    assert_eq!(rendered, "x+1");
}
