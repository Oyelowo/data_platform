use yelang_interner::Interner;
use yelang_macro_core::token_tree::{Delimiter, TokenStream, TokenTree};

use super::cursor::TokenCursor;
use super::fragment_fields;
use super::types::{FragmentFields, FragmentKind};

/// The result of consuming a fragment: the raw captured token stream plus any
/// pre-extracted fragment fields for `$name.field` syntax.
pub struct FragmentCapture {
    pub stream: TokenStream,
    pub fields: Option<FragmentFields>,
}

/// Consume a fragment from the input stream and return its captured token stream.
///
/// The returned stream preserves the original tokens so that hygiene contexts
/// are retained when the capture is substituted into the output.
pub fn consume_fragment(
    cursor: &mut TokenCursor,
    fragment: FragmentKind,
    interner: &Interner,
    repeat_separator: Option<&TokenTree>,
) -> Result<FragmentCapture, String> {
    match fragment {
        FragmentKind::Tt => consume_tt(cursor).map(|s| FragmentCapture {
            stream: s,
            fields: None,
        }),
        FragmentKind::Ident => consume_ident(cursor).map(|s| FragmentCapture {
            stream: s.clone(),
            fields: Some(fragment_fields::from_ident(&s)),
        }),
        FragmentKind::Literal => consume_literal(cursor).map(|s| FragmentCapture {
            stream: s,
            fields: None,
        }),
        FragmentKind::Block => consume_block(cursor, interner).map(|s| FragmentCapture {
            stream: s,
            fields: None,
        }),
        FragmentKind::Expr => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::Expr,
            "expr",
            parse_expr,
            |s| fragment_fields::from_expr(s, interner),
        ),
        FragmentKind::Stmt => consume_stmt(cursor, interner, repeat_separator),
        FragmentKind::Ty => {
            consume_nonterminal(cursor, interner, FragmentKind::Ty, "ty", parse_ty, |s| {
                fragment_fields::from_ty(s, interner)
            })
        }
        FragmentKind::Path => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::Path,
            "path",
            parse_path,
            |_| Ok(FragmentFields::default()),
        ),
        FragmentKind::Item => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::Item,
            "item",
            parse_item,
            |s| fragment_fields::from_item(s, interner),
        ),
        FragmentKind::Pat => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::Pat,
            "pat",
            parse_pat,
            |_| Ok(FragmentFields::default()),
        ),
        FragmentKind::Vis => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::Vis,
            "vis",
            parse_vis,
            |_| Ok(FragmentFields::default()),
        ),
        FragmentKind::Meta => consume_meta(cursor).map(|s| FragmentCapture {
            stream: s,
            fields: None,
        }),
        FragmentKind::Lifetime => consume_lifetime(cursor).map(|s| FragmentCapture {
            stream: s,
            fields: None,
        }),
        FragmentKind::PatParam => consume_nonterminal(
            cursor,
            interner,
            FragmentKind::PatParam,
            "pat_param",
            parse_pat_param,
            |_| Ok(FragmentFields::default()),
        ),
    }
}

fn consume_tt(cursor: &mut TokenCursor) -> Result<TokenStream, String> {
    let tree = cursor
        .advance()
        .ok_or_else(|| "expected token tree".to_string())?;
    Ok(TokenStream::from_vec(vec![tree]))
}

fn consume_ident(cursor: &mut TokenCursor) -> Result<TokenStream, String> {
    match cursor.peek() {
        Some(TokenTree::Ident(_)) => {
            let tree = cursor.advance().unwrap();
            Ok(TokenStream::from_vec(vec![tree]))
        }
        _ => Err("expected identifier".to_string()),
    }
}

fn consume_literal(cursor: &mut TokenCursor) -> Result<TokenStream, String> {
    match cursor.peek() {
        Some(TokenTree::Literal(_)) => {
            let tree = cursor.advance().unwrap();
            Ok(TokenStream::from_vec(vec![tree]))
        }
        _ => Err("expected literal".to_string()),
    }
}

fn consume_meta(cursor: &mut TokenCursor) -> Result<TokenStream, String> {
    let mut taken = Vec::new();

    // Path: ident (:: ident)*
    match cursor.peek() {
        Some(TokenTree::Ident(_)) => taken.push(cursor.advance().unwrap()),
        _ => return Err("expected identifier at start of meta item".to_string()),
    };

    while let (Some(TokenTree::Punct(p)), Some(TokenTree::Punct(p2)), Some(TokenTree::Ident(_))) =
        (cursor.peek(), cursor.peek_ahead(1), cursor.peek_ahead(2))
    {
        if p.ch != ':' || p2.ch != ':' {
            break;
        }
        taken.push(cursor.advance().unwrap()); // ':'
        taken.push(cursor.advance().unwrap()); // ':'
        taken.push(cursor.advance().unwrap()); // ident
    }

    // Optional attribute-style argument group.
    if let Some(TokenTree::Group(_)) = cursor.peek() {
        taken.push(cursor.advance().unwrap());
    }

    if taken.is_empty() {
        return Err("expected meta item".to_string());
    }
    Ok(TokenStream::from_vec(taken))
}

fn consume_lifetime(cursor: &mut TokenCursor) -> Result<TokenStream, String> {
    match (cursor.peek(), cursor.peek_ahead(1)) {
        (Some(TokenTree::Punct(p)), Some(TokenTree::Ident(_))) if p.ch == '\'' => {
            let quote = cursor.advance().unwrap();
            let name = cursor.advance().unwrap();
            Ok(TokenStream::from_vec(vec![quote, name]))
        }
        _ => Err("expected lifetime `'...`".to_string()),
    }
}

fn consume_block(cursor: &mut TokenCursor, interner: &Interner) -> Result<TokenStream, String> {
    match cursor.peek() {
        Some(TokenTree::Group(_)) => {
            let tree = cursor.advance().unwrap();
            if let TokenTree::Group(ref group) = tree {
                if group.delimiter != Delimiter::Brace {
                    return Err("expected block `{ ... }`".to_string());
                }
                // Validate that the contents are a valid block.
                let rendered = tree.render(interner);
                let _ = parse_block(&rendered).map_err(|e| format!("invalid block: {}", e))?;
                Ok(TokenStream::from_vec(vec![tree]))
            } else {
                unreachable!()
            }
        }
        _ => Err("expected block `{ ... }`".to_string()),
    }
}

fn consume_nonterminal<P, T, F>(
    cursor: &mut TokenCursor,
    interner: &Interner,
    fragment: FragmentKind,
    label: &str,
    parse: P,
    extract_fields: F,
) -> Result<FragmentCapture, String>
where
    P: FnOnce(&mut AstTokenStream, &Interner) -> Result<T, String>,
    F: FnOnce(&TokenStream) -> Result<FragmentFields, String>,
{
    let captured = capture_until_separator(cursor, fragment, false);
    if captured.is_empty() {
        return Err(format!("expected {}", label));
    }
    let rendered = captured.render(interner);
    let mut stream = tokenize_for_parse(&rendered, interner)?;
    parse(&mut stream, interner).map_err(|e| format!("invalid {}: {}", label, e))?;
    if !stream.is_eof() {
        return Err(format!("trailing tokens after {}", label));
    }
    let fields = extract_fields(&captured)?;
    Ok(FragmentCapture {
        stream: captured,
        fields: Some(fields),
    })
}

/// Consume a `:stmt` fragment.
///
/// When `:stmt` is inside a repetition whose separator is `;`, the statement
/// stops before the semicolon (which belongs to the repetition).  Otherwise the
/// trailing semicolon is consumed as part of the statement.  In either case the
/// captured stream is validated by parsing it with a trailing semicolon, which
/// is required for `let` statements but does not change the returned tokens.
fn consume_stmt(
    cursor: &mut TokenCursor,
    interner: &Interner,
    repeat_separator: Option<&TokenTree>,
) -> Result<FragmentCapture, String> {
    let stop_at_semicolon = repeat_separator.map(is_semicolon).unwrap_or(false);
    let captured = capture_until_separator(cursor, FragmentKind::Stmt, stop_at_semicolon);
    if captured.is_empty() {
        return Err("expected stmt".to_string());
    }
    let rendered = captured.render(interner);
    // The trailing semicolon is a separator, not part of the statement itself.
    let to_parse = if rendered.ends_with(';') {
        rendered.clone()
    } else {
        format!("{};", rendered)
    };
    let mut stream = tokenize_for_parse(&to_parse, interner)?;
    parse_stmt(&mut stream, interner).map_err(|e| format!("invalid stmt: {}", e))?;
    if !stream.is_eof() {
        return Err("trailing tokens after stmt".to_string());
    }
    Ok(FragmentCapture {
        stream: captured,
        fields: Some(FragmentFields::default()),
    })
}

/// Capture tokens from the cursor until a top-level argument separator.
///
/// For most fragments the separator is `,`.  The `pat_param` fragment also
/// stops at a top-level `|` because or-patterns are not part of
/// `:pat_param`.
///
/// The scanner is delimiter-aware: commas inside `()`, `[]`, and `{}` groups are
/// ignored because those are balanced `TokenTree::Group`s. Commas inside
/// generic argument lists (`<...>`) are also ignored.
///
/// The rules for angle brackets depend on `fragment`:
/// - Type-like fragments (`ty`, `path`, `item`) treat `<` as opening a generic
///   list when it follows an identifier, `::`, or a closing `>`. This allows
///   nested generic types such as `HashMap<String, Vec<i32>>` to be captured as
///   a single fragment.
/// - Expression-like fragments (`expr`, `stmt`, `pat`) only treat `<` as a
///   generic opener when it follows `::` (the turbofish disambiguator). This
///   prevents comparison operators such as `a < b` from swallowing a following
///   argument separator; users can still write turbofish calls like
///   `foo::<T>()`.
///
/// Compound operators starting with `<` (`<=`, `<<`, `<<=`, `<-`, `<->`) are
/// never treated as generic openers.
fn capture_until_separator(
    cursor: &mut TokenCursor,
    fragment: FragmentKind,
    stop_at_semicolon: bool,
) -> TokenStream {
    let type_like = matches!(
        fragment,
        FragmentKind::Ty | FragmentKind::Path | FragmentKind::Item
    );
    let mut taken = Vec::new();
    let mut angle_depth = 0usize;

    while let Some(tree) = cursor.peek() {
        if angle_depth == 0 && is_argument_separator(tree, fragment, stop_at_semicolon) {
            break;
        }

        if let TokenTree::Punct(p) = tree {
            match p.ch {
                '<' if angle_depth_opening(type_like, &taken)
                    && !is_lt_part_of_compound(cursor) =>
                {
                    angle_depth += 1;
                }
                '>' if angle_depth > 0 => {
                    angle_depth = angle_depth.saturating_sub(1);
                }
                _ => {}
            }
        }

        taken.push(cursor.advance().unwrap());
    }

    TokenStream::from_vec(taken)
}

fn is_semicolon(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ';')
}

fn is_argument_separator(
    tree: &TokenTree,
    fragment: FragmentKind,
    stop_at_semicolon: bool,
) -> bool {
    match tree {
        TokenTree::Punct(p) if p.ch == ',' => true,
        // When `:stmt` appears inside a repetition whose separator is `;`, the
        // semicolon is the separator and must not be consumed as part of the
        // statement.
        TokenTree::Punct(p) if p.ch == ';' && stop_at_semicolon => true,
        // `pat_param` stops at top-level `|` because or-patterns are not part
        // of a single `:pat_param` fragment.
        TokenTree::Punct(p) if p.ch == '|' && matches!(fragment, FragmentKind::PatParam) => true,
        _ => false,
    }
}

fn angle_depth_opening(type_like: bool, taken: &[TokenTree]) -> bool {
    let Some(prev) = taken.last() else {
        return false;
    };
    match prev {
        // In expression-like fragments an identifier followed by `<` is a
        // comparison, not a generic opener.
        TokenTree::Ident(_) => type_like,
        TokenTree::Punct(p) => matches!(p.ch, ':' | '>'),
        _ => false,
    }
}

fn is_lt_part_of_compound(cursor: &TokenCursor) -> bool {
    // If the next token is `=`, `<`, or `-`, this `<` is part of `<=`, `<<`,
    // or `<-` (arrow / compound operator), not a generic opening bracket.
    matches!(
        cursor.peek_ahead(1),
        Some(TokenTree::Punct(p)) if p.ch == '=' || p.ch == '<' || p.ch == '-'
    )
}

// --- AST validation helpers ---

fn tokenize_for_parse(
    src: &str,
    interner: &Interner,
) -> Result<yelang_lexer::TokenStream<yelang_ast::tokenizer::TokenKind>, String> {
    yelang_ast::TokenKind::tokenize(src, interner).map_err(|e| e.to_string())
}

type AstTokenStream = yelang_lexer::TokenStream<yelang_ast::tokenizer::TokenKind>;

fn parse_expr(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Expr, String> {
    stream
        .parse::<yelang_ast::Expr>()
        .map_err(|e| e.to_string())
}

fn parse_stmt(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Stmt, String> {
    stream
        .parse::<yelang_ast::Stmt>()
        .map_err(|e| e.to_string())
}

fn parse_ty(stream: &mut AstTokenStream, _interner: &Interner) -> Result<yelang_ast::Type, String> {
    stream
        .parse::<yelang_ast::Type>()
        .map_err(|e| e.to_string())
}

fn parse_path(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Path, String> {
    stream
        .parse::<yelang_ast::Path>()
        .map_err(|e| e.to_string())
}

fn parse_item(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Item, String> {
    stream
        .parse::<yelang_ast::Item>()
        .map_err(|e| e.to_string())
}

fn parse_pat(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Pattern, String> {
    stream
        .parse::<yelang_ast::Pattern>()
        .map_err(|e| e.to_string())
}

fn parse_vis(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::Visibility, String> {
    let vis = stream
        .parse::<yelang_ast::Visibility>()
        .map_err(|e| e.to_string())?;
    if vis.is_private() {
        return Err("expected visibility modifier".to_string());
    }
    Ok(vis)
}

fn parse_pat_param(
    stream: &mut AstTokenStream,
    _interner: &Interner,
) -> Result<yelang_ast::RestrictedPattern, String> {
    stream
        .parse::<yelang_ast::RestrictedPattern>()
        .map_err(|e| e.to_string())
}

fn parse_block(src: &str) -> Result<yelang_ast::BlockExpr, String> {
    let interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &interner).map_err(|e| e.to_string())?;
    stream
        .parse::<yelang_ast::BlockExpr>()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_stream(src: &str, interner: &Interner) -> TokenStream {
        let mut lex = yelang_ast::TokenKind::tokenize(src, interner).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| lex.advance().cloned()).collect();
        yelang_ast::expr::convert::from_lexer_tokens(&tokens, interner)
    }

    fn take_fragment(
        src: &str,
        interner: &Interner,
        fragment: FragmentKind,
    ) -> (TokenStream, TokenStream) {
        let mut cursor = TokenCursor::new(token_stream(src, interner));
        let captured = consume_fragment(&mut cursor, fragment, interner, None).unwrap();
        let remaining = TokenStream::from_vec(cursor.remaining().to_vec());
        (captured.stream, remaining)
    }

    fn take_expr(src: &str, interner: &Interner) -> (TokenStream, TokenStream) {
        take_fragment(src, interner, FragmentKind::Expr)
    }

    fn token_strings(stream: &TokenStream, interner: &Interner) -> Vec<String> {
        stream.iter().map(|t| t.render(interner)).collect()
    }

    fn assert_streams_eq(left: &TokenStream, right: &TokenStream, interner: &Interner) {
        assert_eq!(
            token_strings(left, interner),
            token_strings(right, interner),
            "token streams differ"
        );
    }

    fn assert_remaining_comma(stream: &TokenStream, interner: &Interner) {
        let first = stream.iter().next().expect("expected remaining tokens");
        assert!(
            matches!(first, TokenTree::Punct(p) if p.ch == ','),
            "expected remaining to start with a comma, got {}",
            first.render(interner)
        );
    }

    fn assert_remaining_empty(stream: &TokenStream) {
        assert!(stream.is_empty(), "expected no remaining tokens");
    }

    #[test]
    fn capture_stops_at_top_level_comma() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a, b, c", &interner);
        assert_streams_eq(&captured, &token_stream("a", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_ignores_comma_in_delimited_groups() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("(a, b, c), d", &interner);
        assert_streams_eq(&captured, &token_stream("(a, b, c)", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_ignores_comma_in_array() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("[1, 2, 3], 4", &interner);
        assert_streams_eq(&captured, &token_stream("[1, 2, 3]", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_ignores_comma_in_generic_type() {
        let interner = Interner::new();
        // Test the scanner directly because the type parser used for fragment
        // validation does not currently accept multi-argument generic types.
        let mut cursor = TokenCursor::new(token_stream("HashMap<T, U>, rest", &interner));
        let captured = capture_until_separator(&mut cursor, FragmentKind::Ty, false);
        let remaining = TokenStream::from_vec(cursor.remaining().to_vec());
        assert_streams_eq(
            &captured,
            &token_stream("HashMap<T, U>", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_nested_generic_type_with_comma() {
        let interner = Interner::new();
        // The type parser may not accept the `>>` closing shorthand, so the
        // nested generic is written with an explicit space between closers.
        let (captured, remaining) =
            take_fragment("Vec<Vec<i32> >, rest", &interner, FragmentKind::Ty);
        assert_streams_eq(
            &captured,
            &token_stream("Vec<Vec<i32> >", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_comparison_not_treated_as_generic() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a < b, c", &interner);
        assert_streams_eq(&captured, &token_stream("a < b", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_comparison_without_separator() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a < b", &interner);
        assert_streams_eq(&captured, &token_stream("a < b", &interner), &interner);
        assert_remaining_empty(&remaining);
    }

    #[test]
    fn capture_chained_comparison_with_separator() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a < b && c > d, e", &interner);
        assert_streams_eq(
            &captured,
            &token_stream("a < b && c > d", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_less_equal_not_trapped_in_generic() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a <= b, c", &interner);
        assert_streams_eq(&captured, &token_stream("a <= b", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_shift_left_not_trapped_in_generic() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a << b, c", &interner);
        assert_streams_eq(&captured, &token_stream("a << b", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_shift_left_assign_not_trapped_in_generic() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a <<= b, c", &interner);
        assert_streams_eq(&captured, &token_stream("a <<= b", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_shift_right_comparison() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("a < b >> c, d", &interner);
        assert_streams_eq(&captured, &token_stream("a < b >> c", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_generic_path_with_arguments() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("std::vec::Vec::<i32>::new(), x", &interner);
        assert_streams_eq(
            &captured,
            &token_stream("std::vec::Vec::<i32>::new()", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_nested_tuple_and_array() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("[(1, 2), (3, 4)], 5", &interner);
        assert_streams_eq(
            &captured,
            &token_stream("[(1, 2), (3, 4)]", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_generic_with_comparison_inside_call() {
        let interner = Interner::new();
        let (captured, remaining) = take_expr("foo::<T>(a < b, c > d), x", &interner);
        assert_streams_eq(
            &captured,
            &token_stream("foo::<T>(a < b, c > d)", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_double_comparison_as_two_arguments() {
        let interner = Interner::new();
        // In expression fragments `<` after a plain identifier is treated as a
        // comparison, so the first top-level comma terminates the first arg.
        let (captured, remaining) = take_expr("a < b, c > d, e", &interner);
        assert_streams_eq(&captured, &token_stream("a < b", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_rejects_invalid_expr() {
        let interner = Interner::new();
        let mut cursor = TokenCursor::new(token_stream("@, b", &interner));
        let result = consume_fragment(&mut cursor, FragmentKind::Expr, &interner, None);
        assert!(result.is_err());
    }

    #[test]
    fn capture_vis_pub() {
        let interner = Interner::new();
        let (captured, remaining) = take_fragment("pub, rest", &interner, FragmentKind::Vis);
        assert_streams_eq(&captured, &token_stream("pub", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_vis_pub_crate() {
        let interner = Interner::new();
        let (captured, remaining) = take_fragment("pub(crate), rest", &interner, FragmentKind::Vis);
        assert_streams_eq(&captured, &token_stream("pub(crate)", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_vis_rejects_private() {
        let interner = Interner::new();
        let mut cursor = TokenCursor::new(token_stream("fn, rest", &interner));
        let result = consume_fragment(&mut cursor, FragmentKind::Vis, &interner, None);
        assert!(result.is_err());
    }

    #[test]
    fn capture_meta_path_only() {
        let interner = Interner::new();
        let (captured, remaining) = take_fragment("derive, rest", &interner, FragmentKind::Meta);
        assert_streams_eq(&captured, &token_stream("derive", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_meta_path_with_args() {
        let interner = Interner::new();
        let (captured, remaining) =
            take_fragment("derive(Clone, Debug), rest", &interner, FragmentKind::Meta);
        assert_streams_eq(
            &captured,
            &token_stream("derive(Clone, Debug)", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_meta_qualified_path() {
        let interner = Interner::new();
        let (captured, remaining) =
            take_fragment("std::derive(Foo), rest", &interner, FragmentKind::Meta);
        assert_streams_eq(
            &captured,
            &token_stream("std::derive(Foo)", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_lifetime() {
        let interner = Interner::new();
        let (captured, remaining) = take_fragment("'a, rest", &interner, FragmentKind::Lifetime);
        assert_streams_eq(&captured, &token_stream("'a", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_lifetime_static() {
        let interner = Interner::new();
        let (captured, remaining) =
            take_fragment("'static, rest", &interner, FragmentKind::Lifetime);
        assert_streams_eq(&captured, &token_stream("'static", &interner), &interner);
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_pat_param_allows_pipe_inside_group() {
        let interner = Interner::new();
        let (captured, remaining) =
            take_fragment("Some(x | y), rest", &interner, FragmentKind::PatParam);
        assert_streams_eq(
            &captured,
            &token_stream("Some(x | y)", &interner),
            &interner,
        );
        assert_remaining_comma(&remaining, &interner);
    }

    #[test]
    fn capture_pat_param_stops_at_top_level_pipe() {
        let interner = Interner::new();
        let (captured, remaining) = take_fragment("x | y, rest", &interner, FragmentKind::PatParam);
        assert_streams_eq(&captured, &token_stream("x", &interner), &interner);
        let first = remaining.iter().next().expect("expected remaining tokens");
        assert!(
            matches!(first, TokenTree::Punct(p) if p.ch == '|'),
            "expected remaining to start with a pipe, got {}",
            first.render(&interner)
        );
    }
}
