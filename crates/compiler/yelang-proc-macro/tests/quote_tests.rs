//! Tests for the `quote!` and `quote_spanned!` macros.

use yelang_proc_macro::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree, quote,
    quote_spanned,
};

fn render(ts: TokenStream) -> String {
    ts.to_string()
}

#[test]
fn quote_emits_ident() {
    assert_eq!(render(quote!(foo)), "foo");
}

#[test]
fn quote_emits_punct() {
    assert_eq!(render(quote!(+)), "+");
}

#[test]
fn quote_emits_literal() {
    assert_eq!(render(quote!(42)), "42");
}

#[test]
fn quote_emits_group() {
    assert_eq!(render(quote!((a, b))), "(a, b)");
}

#[test]
fn quote_interpolates_ident() {
    let name = Ident::new("bar", Span::call_site());
    assert_eq!(render(quote!(#name)), "bar");
}

#[test]
fn quote_interpolates_token_stream() {
    let inner: TokenStream = quote!(a + b);
    assert_eq!(render(quote!(#inner)), "a + b");
}

#[test]
fn quote_interpolates_token_tree() {
    let tree = TokenTree::Ident(Ident::new("baz", Span::call_site()));
    assert_eq!(render(quote!(#tree)), "baz");
}

#[test]
fn quote_interpolates_literal() {
    let lit = Literal::integer("7", Span::call_site());
    assert_eq!(render(quote!(#lit)), "7");
}

#[test]
fn quote_interpolates_group() {
    let group = TokenTree::Group(Group::new(
        Delimiter::Parenthesis,
        quote!(x, y),
        Span::call_site(),
    ));
    assert_eq!(render(quote!(#group)), "(x, y)");
}

#[test]
fn quote_interpolates_punct() {
    let punct = TokenTree::Punct(Punct::new('>', Spacing::Alone, Span::call_site()));
    assert_eq!(render(quote!(#punct)), ">");
}

#[test]
fn quote_string_literal() {
    assert_eq!(render(quote!("hello")), "\"hello\"");
}

#[test]
fn quote_integer_literal() {
    assert_eq!(render(quote!(123)), "123");
}

#[test]
fn quote_group_with_interpolation() {
    let name = Ident::new("foo", Span::call_site());
    // The renderer inserts a space between an identifier and a following group.
    assert_eq!(render(quote!(#name())), "foo ()");
}

#[test]
fn quote_interpolates_grouped_expression() {
    let expr: TokenStream = quote!(a + b);
    assert_eq!(render(quote!((#expr))), "(a + b)");
}

#[test]
fn quote_repeats_without_separator() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items)*)), "a b");
}

#[test]
fn quote_repeats_with_comma_separator() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items),*)), "a, b");
}

#[test]
fn quote_repeats_with_semicolon_separator() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items);*)), "a; b");
}

#[test]
fn quote_repeats_plus_one_or_more() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items)+)), "a b");
}

#[test]
#[should_panic(expected = "`quote!` `+` repetition requires a non-empty iterator")]
fn quote_repeats_plus_requires_non_empty() {
    let items: Vec<Ident> = Vec::new();
    let _ = render(quote!(#(#items)+));
}

#[test]
fn quote_repeats_empty_star_yields_nothing() {
    let items: Vec<Ident> = Vec::new();
    assert_eq!(render(quote!(#(#items)*)), "");
}

#[test]
fn quote_repeats_with_multiple_interpolations() {
    let names = vec![
        Ident::new("x", Span::call_site()),
        Ident::new("y", Span::call_site()),
    ];
    let tys = vec![
        Ident::new("i32", Span::call_site()),
        Ident::new("u32", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#names: #tys),*)), "x: i32, y: u32");
}

#[test]
fn quote_repeats_with_multiple_interpolations_and_separator() {
    let names = vec![
        Ident::new("x", Span::call_site()),
        Ident::new("y", Span::call_site()),
    ];
    let tys = vec![
        Ident::new("i32", Span::call_site()),
        Ident::new("u32", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#names = #tys);*)), "x = i32; y = u32");
}

#[test]
fn quote_repeats_with_referenced_iterable() {
    let items = [
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items),*)), "a, b");
}

#[test]
fn quote_repeats_with_option() {
    let opt = Some(Ident::new("a", Span::call_site()));
    assert_eq!(render(quote!(#(#opt),*)), "a");
}

#[test]
fn quote_repeats_with_slice() {
    let items: &[Ident] = &[
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items),*)), "a, b");
}

#[test]
fn quote_repeats_with_vec() {
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
        Ident::new("c", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#items),*)), "a, b, c");
}

#[test]
fn quote_nested_repetitions_with_separators() {
    // Both repetitions iterate over the same collection; the inner repetition is
    // evaluated once per outer element. This documents the current behaviour
    // rather than claiming full matrix nesting.
    let items = vec![
        Ident::new("a", Span::call_site()),
        Ident::new("b", Span::call_site()),
    ];
    assert_eq!(render(quote!(#(#(#items),*);*)), "a, b; a, b");
}

#[test]
fn quote_nested_repetitions_mixed_star_plus() {
    let items = vec![Ident::new("a", Span::call_site())];
    assert_eq!(render(quote!(#(#(#items)+)*)), "a");
}

#[test]
fn quote_empty_invocation_yields_empty_stream() {
    assert_eq!(render(quote!()), "");
}

#[test]
fn quote_spanned_applies_span_to_originating_tokens() {
    let span = Span::call_site();
    let name = Ident::new("foo", Span::call_site());
    let ts = quote_spanned!(span=> #name: Copy);
    // Rendering does not expose spans; this primarily checks that the macro
    // expands without errors and produces the expected tokens.
    assert_eq!(render(ts), "foo: Copy");
}

#[test]
fn quote_spanned_preserves_interpolated_spans() {
    let span = Span::call_site();
    let inner = quote!(a + b);
    assert_eq!(render(quote_spanned!(span=> #inner)), "a + b");
}

#[test]
fn quote_spanned_with_delim_span() {
    let span = Span::call_site();
    assert_eq!(render(quote_spanned!(span=> { 1 + 1 })), "{1 + 1}");
}

#[test]
fn quote_spanned_with_expr_span() {
    let span = Span::call_site();
    let name = Ident::new("bar", Span::call_site());
    assert_eq!(render(quote_spanned!(span=> #name())), "bar ()");
}
