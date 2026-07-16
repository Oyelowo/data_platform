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
        FragmentKind::Expr => consume_nonterminal(cursor, interner, "expr", parse_expr, |s| {
            fragment_fields::from_expr(s, interner)
        }),
        FragmentKind::Stmt => consume_nonterminal(cursor, interner, "stmt", parse_stmt, |_| {
            Ok(FragmentFields::default())
        }),
        FragmentKind::Ty => consume_nonterminal(cursor, interner, "ty", parse_ty, |s| {
            fragment_fields::from_ty(s, interner)
        }),
        FragmentKind::Path => consume_nonterminal(cursor, interner, "path", parse_path, |_| {
            Ok(FragmentFields::default())
        }),
        FragmentKind::Item => consume_nonterminal(cursor, interner, "item", parse_item, |s| {
            fragment_fields::from_item(s, interner)
        }),
        FragmentKind::Pat => consume_nonterminal(cursor, interner, "pat", parse_pat, |_| {
            Ok(FragmentFields::default())
        }),
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
    label: &str,
    parse: P,
    extract_fields: F,
) -> Result<FragmentCapture, String>
where
    P: FnOnce(&str, &Interner) -> Result<T, String>,
    F: FnOnce(&TokenStream) -> Result<FragmentFields, String>,
{
    let captured = capture_until_separator(cursor);
    if captured.is_empty() {
        return Err(format!("expected {}", label));
    }
    let rendered = captured.render(interner);
    parse(&rendered, interner).map_err(|e| format!("invalid {}: {}", label, e))?;
    let fields = extract_fields(&captured)?;
    Ok(FragmentCapture {
        stream: captured,
        fields: Some(fields),
    })
}

/// Capture tokens from the cursor until a top-level argument separator (`,`)
/// or the end of the current group.
fn capture_until_separator(cursor: &mut TokenCursor) -> TokenStream {
    let mut taken = Vec::new();
    while let Some(tree) = cursor.peek() {
        if is_argument_separator(tree) {
            break;
        }
        taken.push(cursor.advance().unwrap());
    }
    TokenStream::from_vec(taken)
}

fn is_argument_separator(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ',')
}

// --- AST validation helpers ---

fn tokenize_for_parse(
    src: &str,
    interner: &Interner,
) -> Result<yelang_lexer::TokenStream<yelang_ast::tokenizer::TokenKind>, String> {
    yelang_ast::TokenKind::tokenize(src, interner).map_err(|e| e.to_string())
}

fn parse_expr(src: &str, interner: &Interner) -> Result<yelang_ast::Expr, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Expr>()
        .map_err(|e| e.to_string())
}

fn parse_stmt(src: &str, interner: &Interner) -> Result<yelang_ast::Stmt, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Stmt>()
        .map_err(|e| e.to_string())
}

fn parse_ty(src: &str, interner: &Interner) -> Result<yelang_ast::Type, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Type>()
        .map_err(|e| e.to_string())
}

fn parse_path(src: &str, interner: &Interner) -> Result<yelang_ast::Path, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Path>()
        .map_err(|e| e.to_string())
}

fn parse_item(src: &str, interner: &Interner) -> Result<yelang_ast::Item, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Item>()
        .map_err(|e| e.to_string())
}

fn parse_pat(src: &str, interner: &Interner) -> Result<yelang_ast::Pattern, String> {
    let mut stream = tokenize_for_parse(src, interner)?;
    stream
        .parse::<yelang_ast::Pattern>()
        .map_err(|e| e.to_string())
}

fn parse_block(src: &str) -> Result<yelang_ast::BlockExpr, String> {
    let mut interner = Interner::new();
    let mut stream =
        yelang_ast::TokenKind::tokenize(src, &mut interner).map_err(|e| e.to_string())?;
    stream
        .parse::<yelang_ast::BlockExpr>()
        .map_err(|e| e.to_string())
}
