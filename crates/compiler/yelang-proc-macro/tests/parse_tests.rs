//! Tests for the public parse helpers.

use yelang_proc_macro::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
    parse::{BufferedCursor, Cursor, ParseError, Parser},
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

fn is_ident(tree: &TokenTree, name: &str) -> bool {
    matches!(tree, TokenTree::Ident(i) if i.value() == name)
}

fn is_literal(tree: &TokenTree, text: &str) -> bool {
    matches!(tree, TokenTree::Literal(l) if l.to_string() == text)
}

fn is_punct(tree: &TokenTree, ch: char) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.as_char() == ch)
}

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

#[test]
fn cursor_peek_and_next() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut cursor = Cursor::new(&s);

    assert!(is_ident(cursor.peek().unwrap(), "a"));
    assert!(is_ident(cursor.peek_n(2).unwrap(), "c"));
    assert!(is_ident(&cursor.next().unwrap(), "a"));
    assert!(is_ident(&cursor.next().unwrap(), "b"));
    assert!(is_ident(cursor.peek().unwrap(), "c"));
    assert!(is_ident(&cursor.next().unwrap(), "c"));
    assert_eq!(cursor.next(), None);
    assert!(cursor.is_empty());
}

#[test]
fn cursor_advance_and_remaining() {
    let s = stream([ident("a"), ident("b"), ident("c"), ident("d")]);
    let mut cursor = Cursor::new(&s);

    cursor.advance(2);
    assert_eq!(cursor.position(), 2);
    assert_eq!(cursor.remaining(), 2);
    assert!(is_ident(&cursor.next().unwrap(), "c"));

    cursor.advance(10);
    assert!(cursor.is_empty());
}

#[test]
fn cursor_fork_is_independent() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut cursor = Cursor::new(&s);
    let mut fork = cursor.fork();

    fork.next();
    assert_eq!(cursor.position(), 0);
    assert_eq!(fork.position(), 1);

    cursor.advance(2);
    assert_eq!(cursor.position(), 2);
    assert_eq!(fork.position(), 1);
}

#[test]
fn cursor_remaining_stream() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut cursor = Cursor::new(&s);
    cursor.next();

    let remaining: TokenStream = cursor.remaining_stream();
    assert_eq!(remaining.len(), 2);
}

#[test]
fn cursor_parse_builtin_types() {
    let s = stream([ident("foo"), lit("42"), punct('+', Spacing::Alone)]);
    let mut cursor = Cursor::new(&s);

    let i: Ident = cursor.parse().unwrap();
    assert_eq!(i.value(), "foo");

    let l: Literal = cursor.parse().unwrap();
    assert_eq!(l.to_string(), "42");

    let p: Punct = cursor.parse().unwrap();
    assert_eq!(p.as_char(), '+');
}

#[test]
fn cursor_parse_error_on_mismatch() {
    let s = stream([ident("foo")]);
    let mut cursor = Cursor::new(&s);

    let result: Result<Literal, ParseError> = cursor.parse();
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

#[test]
fn parser_parse_builtin_types() {
    let s = stream([ident("foo"), lit("42")]);
    let mut parser = Parser::new(&s);

    let i: Ident = parser.parse().unwrap();
    assert_eq!(i.value(), "foo");

    let l: Literal = parser.parse().unwrap();
    assert_eq!(l.to_string(), "42");
}

#[test]
fn parser_peek_and_next() {
    let s = stream([ident("a"), ident("b")]);
    let mut parser = Parser::new(&s);

    assert!(is_ident(parser.peek().unwrap(), "a"));
    assert!(is_ident(parser.peek_n(1).unwrap(), "b"));
    assert!(is_ident(&parser.next().unwrap(), "a"));
    assert!(is_ident(&parser.next().unwrap(), "b"));
}

#[test]
fn parser_expect() {
    let s = stream([ident("a")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect("any token").is_ok());
    assert!(parser.expect("any token").is_err());
}

#[test]
fn parser_expect_ident() {
    let s = stream([ident("a"), lit("1")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect_ident().is_ok());
    assert!(parser.expect_ident().is_err());
}

#[test]
fn parser_expect_punct() {
    let s = stream([punct('+', Spacing::Alone), ident("a")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect_punct('+').is_ok());
    assert!(parser.expect_punct('+').is_err());
}

#[test]
fn parser_expect_literal() {
    let s = stream([lit("42"), ident("a")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect_literal().is_ok());
    assert!(parser.expect_literal().is_err());
}

#[test]
fn parser_expect_group() {
    let inner = stream([ident("x")]);
    let group = Group::new(Delimiter::Parenthesis, inner, Span::call_site());
    let s = stream([TokenTree::Group(group), ident("a")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect_group(Delimiter::Parenthesis).is_ok());
    assert!(parser.expect_group(Delimiter::Parenthesis).is_err());
}

#[test]
fn parser_expect_keyword() {
    let s = stream([ident("struct"), ident("foo")]);
    let mut parser = Parser::new(&s);

    assert!(parser.expect_keyword("struct").is_ok());
    assert!(parser.expect_keyword("struct").is_err());
}

#[test]
fn parser_matches_and_consume() {
    let s = stream([punct('+', Spacing::Alone), ident("foo")]);
    let mut parser = Parser::new(&s);

    assert!(parser.matches_punct('+'));
    assert!(!parser.matches_punct('-'));
    assert!(parser.consume_punct('+'));
    assert!(!parser.consume_punct('+'));

    assert!(parser.matches_ident("foo"));
    assert!(parser.consume_ident("foo"));
    assert!(!parser.consume_ident("foo"));
}

#[test]
fn parser_parse_optional() {
    let s = stream([ident("a"), lit("1")]);
    let mut parser = Parser::new(&s);

    let first: Option<Ident> = parser.parse_optional();
    assert!(first.is_some());

    let second: Option<Ident> = parser.parse_optional();
    assert!(second.is_none());

    // Cursor should be restored after the failed optional parse.
    let lit: Literal = parser.parse().unwrap();
    assert_eq!(lit.to_string(), "1");
}

#[test]
fn parser_parse_group() {
    let inner = stream([ident("x")]);
    let group = Group::new(Delimiter::Parenthesis, inner, Span::call_site());
    let s = stream([TokenTree::Group(group)]);
    let mut parser = Parser::new(&s);

    let g: Group = parser.parse_group().unwrap();
    assert_eq!(g.delimiter(), Delimiter::Parenthesis);
}

#[test]
fn parser_parse_terminated_without_separator() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut parser = Parser::new(&s);

    let items: Vec<Ident> = parser.parse_terminated(|t| is_ident(t, "c"), None).unwrap();
    assert_eq!(items.len(), 2);
}

#[test]
fn parser_parse_terminated_with_separator() {
    let s = stream([
        ident("a"),
        punct(',', Spacing::Alone),
        ident("b"),
        punct(',', Spacing::Alone),
        ident("end"),
    ]);
    let mut parser = Parser::new(&s);

    let items: Vec<Ident> = parser
        .parse_terminated(|t| is_ident(t, "end"), Some(','))
        .unwrap();
    assert_eq!(items.len(), 2);
}

#[test]
fn parser_parse_many0_and_many1() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut parser = Parser::new(&s);
    let items: Vec<Ident> = parser.parse_many0().unwrap();
    assert_eq!(items.len(), 3);

    let empty = stream([]);
    let mut parser = Parser::new(&empty);
    let items: Vec<Ident> = parser.parse_many0().unwrap();
    assert_eq!(items.len(), 0);

    let mut parser = Parser::new(&empty);
    assert!(parser.parse_many1::<Ident>().is_err());
}

#[test]
fn parser_parse_separated0_and_separated1() {
    let s = stream([
        ident("a"),
        punct(',', Spacing::Alone),
        ident("b"),
        punct(',', Spacing::Alone),
    ]);
    let mut parser = Parser::new(&s);
    let items: Vec<Ident> = parser.parse_separated0(',').unwrap();
    assert_eq!(items.len(), 2);

    let empty = stream([]);
    let mut parser = Parser::new(&empty);
    let items: Vec<Ident> = parser.parse_separated0(',').unwrap();
    assert_eq!(items.len(), 0);

    let mut parser = Parser::new(&empty);
    assert!(parser.parse_separated1::<Ident>(',').is_err());
}

#[test]
fn parser_parse_delimited() {
    let inner = stream([ident("x"), punct('+', Spacing::Alone), lit("1")]);
    let group = Group::new(Delimiter::Parenthesis, inner, Span::call_site());
    let s = stream([TokenTree::Group(group)]);
    let mut parser = Parser::new(&s);

    let mut inner_parser = parser.parse_delimited(Delimiter::Parenthesis).unwrap();
    let _: Ident = inner_parser.parse().unwrap();
    let _: Punct = inner_parser.parse().unwrap();
    let _: Literal = inner_parser.parse().unwrap();
    assert!(inner_parser.is_empty());
}

// ---------------------------------------------------------------------------
// BufferedCursor
// ---------------------------------------------------------------------------

#[test]
fn buffered_cursor_fork_and_commit() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut buf = BufferedCursor::new(Cursor::new(&s));

    let mut fork = buf.fork();
    fork.advance(2);
    assert_eq!(buf.cursor().position(), 0);

    buf.commit(fork);
    assert_eq!(buf.cursor().position(), 2);
}

#[test]
fn buffered_cursor_reset() {
    let s = stream([ident("a"), ident("b"), ident("c")]);
    let mut buf = BufferedCursor::new(Cursor::new(&s));
    let snapshot = buf.fork();

    buf.cursor_mut().advance(2);
    assert_eq!(buf.cursor().position(), 2);

    buf.reset(snapshot);
    assert_eq!(buf.cursor().position(), 0);
}

#[test]
fn buffered_cursor_into_cursor() {
    let s = stream([ident("a")]);
    let buf = BufferedCursor::new(Cursor::new(&s));
    let cursor = buf.into_cursor();
    assert_eq!(cursor.position(), 0);
}

// ---------------------------------------------------------------------------
// Parse impl edge cases
// ---------------------------------------------------------------------------

#[test]
fn parse_token_stream_returns_remaining() {
    let s = stream([ident("a"), ident("b")]);
    let mut cursor = Cursor::new(&s);
    cursor.next();

    let remaining: TokenStream = cursor.parse().unwrap();
    assert_eq!(remaining.len(), 1);
}

#[test]
fn parse_token_tree_returns_any_tree() {
    let s = stream([ident("a"), lit("1")]);
    let mut cursor = Cursor::new(&s);

    let first: TokenTree = cursor.parse().unwrap();
    assert!(is_ident(&first, "a"));

    let second: TokenTree = cursor.parse().unwrap();
    assert!(is_literal(&second, "1"));
}

#[test]
fn parse_group_rejects_non_group() {
    let s = stream([ident("a")]);
    let mut cursor = Cursor::new(&s);
    assert!(cursor.parse::<Group>().is_err());
}

#[test]
fn parse_punct_rejects_non_punct() {
    let s = stream([ident("a")]);
    let mut cursor = Cursor::new(&s);
    assert!(cursor.parse::<Punct>().is_err());
}
