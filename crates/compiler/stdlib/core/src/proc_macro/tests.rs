//! Exhaustive unit tests for the Yelang `proc_macro` standard-library API.

use yelang_proc_macro::{
    Delimiter, Diagnostic, Group, Ident, Level, Literal, Punct, Spacing, Span, TokenStream,
    TokenTree,
};

fn call_site() -> Span {
    Span::call_site()
}

#[test]
fn token_stream_new_is_empty() {
    let stream = TokenStream::new();
    assert!(stream.is_empty());
    assert_eq!(stream.len(), 0);
}

#[test]
fn token_stream_push_and_len() {
    let mut stream = TokenStream::new();
    stream.push(TokenTree::Ident(Ident::new("x", call_site())));
    assert!(!stream.is_empty());
    assert_eq!(stream.len(), 1);
}

#[test]
fn token_stream_extend() {
    let mut a = TokenStream::new();
    a.push(TokenTree::Ident(Ident::new("a", call_site())));

    let mut b = TokenStream::new();
    b.push(TokenTree::Ident(Ident::new("b", call_site())));
    b.push(TokenTree::Ident(Ident::new("c", call_site())));

    a.extend(b);
    assert_eq!(a.len(), 3);
}

#[test]
fn token_stream_from_token_tree() {
    let tree = TokenTree::Ident(Ident::new("foo", call_site()));
    let stream: TokenStream = tree.into();
    assert_eq!(stream.len(), 1);
}

#[test]
fn token_stream_from_iterator() {
    let stream: TokenStream = vec![
        TokenTree::Ident(Ident::new("a", call_site())),
        TokenTree::Ident(Ident::new("b", call_site())),
    ]
    .into_iter()
    .collect();
    assert_eq!(stream.len(), 2);
}

#[test]
fn token_stream_iter() {
    let mut stream = TokenStream::new();
    stream.push(TokenTree::Ident(Ident::new("x", call_site())));
    let collected: Vec<_> = stream.iter().collect();
    assert_eq!(collected.len(), 1);
}

#[test]
fn token_stream_into_iter() {
    let mut stream = TokenStream::new();
    stream.push(TokenTree::Ident(Ident::new("x", call_site())));
    let count = stream.into_iter().count();
    assert_eq!(count, 1);
}

#[test]
fn ident_creation_and_value() {
    let ident = Ident::new("my_ident", call_site());
    assert_eq!(ident.value(), "my_ident");
}

#[test]
fn ident_span_round_trip() {
    let span = call_site();
    let ident = Ident::new("x", span);
    assert_eq!(ident.span(), span);
}

#[test]
fn ident_with_span() {
    let ident = Ident::new("x", call_site()).with_span(call_site());
    assert_eq!(ident.value(), "x");
}

#[test]
fn literal_string() {
    let lit = Literal::string("hello", call_site());
    assert_eq!(lit.to_string(), "\"hello\"");
}

#[test]
fn literal_integer() {
    let lit = Literal::integer("42", call_site());
    assert_eq!(lit.to_string(), "42");
}

#[test]
fn literal_integer_with_suffix() {
    let lit = Literal::integer("42u32", call_site());
    assert_eq!(lit.to_string(), "42u32");
}

#[test]
fn literal_float() {
    let lit = Literal::float("3.14", call_site());
    assert_eq!(lit.to_string(), "3.14");
}

#[test]
fn literal_character() {
    let lit = Literal::character('x', call_site());
    assert_eq!(lit.to_string(), "'x'");
}

#[test]
fn literal_boolean() {
    let lit_true = Literal::boolean(true, call_site());
    let lit_false = Literal::boolean(false, call_site());
    assert_eq!(lit_true.to_string(), "true");
    assert_eq!(lit_false.to_string(), "false");
}

#[test]
fn literal_byte_string() {
    let lit = Literal::byte_string("bytes", call_site());
    assert_eq!(lit.to_string(), "b\"bytes\"");
}

#[test]
fn literal_byte() {
    let lit = Literal::byte(b'x', call_site());
    assert_eq!(lit.to_string(), "b'x'");
}

#[test]
fn literal_span_round_trip() {
    let span = call_site();
    let lit = Literal::string("s", span);
    assert_eq!(lit.span(), span);
}

#[test]
fn punct_creation_and_accessors() {
    let p = Punct::new('+', Spacing::Alone, call_site());
    assert_eq!(p.as_char(), '+');
    assert_eq!(p.spacing(), Spacing::Alone);
}

#[test]
fn punct_joint_spacing() {
    let p = Punct::new(':', Spacing::Joint, call_site());
    assert_eq!(p.spacing(), Spacing::Joint);
}

#[test]
fn punct_with_span() {
    let p = Punct::new('+', Spacing::Alone, call_site()).with_span(call_site());
    assert_eq!(p.as_char(), '+');
}

#[test]
fn group_creation_and_accessors() {
    let mut inner = TokenStream::new();
    inner.push(TokenTree::Ident(Ident::new("x", call_site())));

    let group = Group::new(Delimiter::Parenthesis, inner, call_site());
    assert_eq!(group.delimiter(), Delimiter::Parenthesis);
    assert_eq!(group.stream().len(), 1);
}

#[test]
fn group_all_delimiters() {
    for delim in [
        Delimiter::Parenthesis,
        Delimiter::Brace,
        Delimiter::Bracket,
        Delimiter::None,
    ] {
        let group = Group::new(delim, TokenStream::new(), call_site());
        assert_eq!(group.delimiter(), delim);
    }
}

#[test]
fn token_tree_variants() {
    let ident = TokenTree::Ident(Ident::new("x", call_site()));
    let punct = TokenTree::Punct(Punct::new('+', Spacing::Alone, call_site()));
    let lit = TokenTree::Literal(Literal::integer("1", call_site()));
    let group = TokenTree::Group(Group::new(
        Delimiter::Brace,
        TokenStream::new(),
        call_site(),
    ));

    assert!(matches!(ident, TokenTree::Ident(_)));
    assert!(matches!(punct, TokenTree::Punct(_)));
    assert!(matches!(lit, TokenTree::Literal(_)));
    assert!(matches!(group, TokenTree::Group(_)));
}

#[test]
fn span_call_site_is_stable() {
    let a = Span::call_site();
    let b = Span::call_site();
    assert_eq!(a, b);
}

#[test]
fn span_def_and_mixed_site_exist() {
    let _ = Span::def_site();
    let _ = Span::mixed_site();
}

#[test]
fn source_file_has_path() {
    let sf = Span::call_site().source_file();
    assert!(!sf.path().is_empty());
}

#[test]
fn line_column_has_non_zero_column() {
    let lc = Span::call_site().start();
    assert!(lc.column > 0);
}

#[test]
fn diagnostic_levels() {
    let e = Diagnostic::error("e");
    let w = Diagnostic::warning("w");
    let n = Diagnostic::note("n");
    let h = Diagnostic::help("h");

    assert!(matches!(e.level, Level::Error));
    assert!(matches!(w.level, Level::Warning));
    assert!(matches!(n.level, Level::Note));
    assert!(matches!(h.level, Level::Help));
}

#[test]
fn diagnostic_span_attachment() {
    let d = Diagnostic::error("msg").span(call_site());
    assert!(!d.spans.is_empty());
}

#[test]
fn token_stream_core_conversion_round_trip() {
    let mut stream = TokenStream::new();
    stream.push(TokenTree::Ident(Ident::new("foo", call_site())));
    stream.push(TokenTree::Punct(Punct::new(
        ':',
        Spacing::Joint,
        call_site(),
    )));
    stream.push(TokenTree::Punct(Punct::new(
        ':',
        Spacing::Joint,
        call_site(),
    )));
    stream.push(TokenTree::Ident(Ident::new("bar", call_site())));

    // Convert to the compiler's canonical token-tree representation and back
    // through a single interner so that symbol lookups remain valid.
    let interner = yelang_interner::Interner::new();
    let core = stream.clone().into_core_stream_with_interner(&interner);
    let round = TokenStream::from_core_stream(&core, &interner);
    assert_eq!(round.len(), stream.len());

    // Also verify the rendered source round-trips.
    let original_source = stream.render_source(&interner);
    let round_source = round.render_source(&interner);
    assert_eq!(original_source, round_source);
}

#[test]
fn token_stream_display_preserves_structure() {
    let mut stream = TokenStream::new();
    stream.push(TokenTree::Ident(Ident::new("fn", call_site())));
    stream.push(TokenTree::Ident(Ident::new("foo", call_site())));
    stream.push(TokenTree::Group(Group::new(
        Delimiter::Parenthesis,
        TokenStream::new(),
        call_site(),
    )));

    let rendered = format!("{}", stream);
    assert!(rendered.contains("fn"));
    assert!(rendered.contains("foo"));
    assert!(rendered.contains("("));
    assert!(rendered.contains(")"));
}

#[test]
fn literal_from_source_text_string() {
    let lit = Literal::from_source_text("\"hello world\"", call_site());
    assert_eq!(lit.to_string(), "\"hello world\"");
}

#[test]
fn literal_from_source_text_integer() {
    let lit = Literal::from_source_text("123", call_site());
    assert_eq!(lit.to_string(), "123");
}

#[test]
fn literal_from_source_text_float() {
    let lit = Literal::from_source_text("3.14", call_site());
    assert_eq!(lit.to_string(), "3.14");
}
