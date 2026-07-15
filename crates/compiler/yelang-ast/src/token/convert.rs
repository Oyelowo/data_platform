use crate::tokenizer::TokenKind as AstTokenKind;
use yelang_lexer::{Literal as LexerLiteral, Token};

use super::{
    Delimiter, Group, Ident, LitKind, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
};

/// Convert a sequence of lexer tokens into a macro `TokenStream`.
///
/// This handles balanced delimiters by building nested `Group`s and expands
/// compound punctuation into the correct sequence of `Punct` tokens so that
/// the stream can be round-tripped back to source text.
pub fn from_lexer_tokens(tokens: &[Token<AstTokenKind>]) -> TokenStream {
    let mut stream = TokenStream::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((trees, consumed)) = convert_token(tokens, i) {
            for tree in trees {
                stream.push(tree);
            }
            i += consumed;
        } else {
            // Skip whitespace, comments, and unrecognized tokens.
            i += 1;
        }
    }
    stream
}

fn convert_token(tokens: &[Token<AstTokenKind>], start: usize) -> Option<(Vec<TokenTree>, usize)> {
    let token = tokens.get(start)?;
    let span: Span = token.span().into();

    match token.kind() {
        AstTokenKind::Ident(ident) => {
            Some((vec![TokenTree::Ident(Ident::new(ident.symbol, span))], 1))
        }
        AstTokenKind::Lit(lit) => {
            convert_literal(lit, span).map(|l| (vec![TokenTree::Literal(l)], 1))
        }
        AstTokenKind::OpenParen => {
            let (inner, consumed) = read_delimited(tokens, start, Delimiter::Parenthesis)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        AstTokenKind::OpenBrace => {
            let (inner, consumed) = read_delimited(tokens, start, Delimiter::Brace)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        AstTokenKind::OpenBracket => {
            let (inner, consumed) = read_delimited(tokens, start, Delimiter::Bracket)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        other => convert_punct(other, span).map(|trees| (trees, 1)),
    }
}

fn read_delimited(
    tokens: &[Token<AstTokenKind>],
    start: usize,
    delimiter: Delimiter,
) -> Option<(Group, usize)> {
    let open = tokens.get(start)?;
    let close_kind = close_kind(delimiter)?;
    let mut depth = 1;
    let mut i = start + 1;
    while i < tokens.len() {
        let token = &tokens[i];
        if opens_delimiter(token.kind()) == Some(delimiter) {
            depth += 1;
        } else if token.kind() == &close_kind {
            depth -= 1;
            if depth == 0 {
                let inner_tokens = &tokens[start + 1..i];
                let inner = from_lexer_tokens(inner_tokens);
                let merged = open.span().merge(token.span());
                let span: Span = merged.into();
                return Some((Group::new(delimiter, inner, span), i - start + 1));
            }
        }
        i += 1;
    }
    None
}

fn opens_delimiter(kind: &AstTokenKind) -> Option<Delimiter> {
    match kind {
        AstTokenKind::OpenParen => Some(Delimiter::Parenthesis),
        AstTokenKind::OpenBrace => Some(Delimiter::Brace),
        AstTokenKind::OpenBracket => Some(Delimiter::Bracket),
        _ => None,
    }
}

fn close_kind(delimiter: Delimiter) -> Option<AstTokenKind> {
    match delimiter {
        Delimiter::Parenthesis => Some(AstTokenKind::CloseParen),
        Delimiter::Brace => Some(AstTokenKind::CloseBrace),
        Delimiter::Bracket => Some(AstTokenKind::CloseBracket),
        Delimiter::None => None,
    }
}

fn convert_literal(lit: &LexerLiteral, span: Span) -> Option<Literal> {
    match lit {
        LexerLiteral::Int(i) => Some(Literal::new(
            LitKind::Int {
                value: i.value,
                suffix: i.suffix.map(|s| s.to_string()),
            },
            span,
        )),
        LexerLiteral::Float(f) => Some(Literal::new(
            LitKind::Float {
                value: f.value,
                suffix: f.suffix.map(|s| s.to_string()),
            },
            span,
        )),
        LexerLiteral::Bool(b) => Some(Literal::bool(*b, span)),
        LexerLiteral::Char(c) => Some(Literal::char(*c, span)),
        LexerLiteral::Str(s) => Some(Literal::string(s.value, span)),
        _ => None,
    }
}

/// Convert a single lexer punctuation token into one or more macro `Punct` tokens.
///
/// Compound operators are expanded so that they round-trip correctly when
/// rendered back to source (e.g. `<=` becomes `<` + `=`).
fn convert_punct(kind: &AstTokenKind, span: Span) -> Option<Vec<TokenTree>> {
    use AstTokenKind::*;

    let joint = Spacing::Joint;
    let alone = Spacing::Alone;

    let chars = match kind {
        Dot => vec![('.', alone)],
        Comma => vec![(',', alone)],
        Colon => vec![(':', joint)],
        Semicolon => vec![(';', alone)],
        Plus => vec![('+', joint)],
        Minus => vec![('-', joint)],
        Star => vec![('*', joint)],
        Slash => vec![('/', joint)],
        Percent => vec![('%', joint)],
        Caret => vec![('^', joint)],
        Ampersand => vec![('&', joint)],
        Pipe => vec![('|', joint)],
        Bang => vec![('!', joint)],
        LessThan => vec![('<', joint)],
        GreaterThan => vec![('>', joint)],
        Equal => vec![('=', joint)],
        // Compound operators.
        DotDotDot => vec![('.', joint), ('.', joint), ('.', alone)],
        DotDotEq => vec![('.', joint), ('.', joint), ('=', alone)],
        ShiftLeftEqual => vec![('<', joint), ('<', joint), ('=', alone)],
        ShiftRightEqual => vec![('>', joint), ('>', joint), ('=', alone)],
        PlusEqual => vec![('+', joint), ('=', alone)],
        MinusEqual => vec![('-', joint), ('=', alone)],
        StarEqual => vec![('*', joint), ('=', alone)],
        SlashEqual => vec![('/', joint), ('=', alone)],
        PercentEqual => vec![('%', joint), ('=', alone)],
        CaretEqual => vec![('^', joint), ('=', alone)],
        AmpersandEqual => vec![('&', joint), ('=', alone)],
        PipeEqual => vec![('|', joint), ('=', alone)],
        LessThanEqual => vec![('<', joint), ('=', alone)],
        GreaterThanEqual => vec![('>', joint), ('=', alone)],
        EqualEqual => vec![('=', joint), ('=', alone)],
        BangEqual => vec![('!', joint), ('=', alone)],
        ArrowBoth => vec![('<', joint), ('-', joint), ('>', alone)],
        ArrowLeft => vec![('<', joint), ('-', alone)],
        ArrowRight => vec![('-', joint), ('>', alone)],
        ArrowRight2Lines => vec![('=', joint), ('>', alone)],
        DotDot => vec![('.', joint), ('.', alone)],
        ColonColon => vec![(':', joint), (':', alone)],
        _ => return None,
    };

    Some(
        chars
            .into_iter()
            .map(|(ch, spacing)| TokenTree::Punct(Punct::new(ch, spacing, span)))
            .collect(),
    )
}
