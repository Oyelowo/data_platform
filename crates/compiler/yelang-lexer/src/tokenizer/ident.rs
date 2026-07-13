/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use super::TokenKind;
use crate::{Interner, Symbol};
use crate::{ParseTokenStream, Span, TokenResult, TokenStream, consume_token};

#[derive(Debug, Eq, Clone, Copy)]
pub struct Ident {
    pub symbol: Symbol,
    pub span: Span,
}

impl Ident {
    pub fn new(symbol: Symbol, span: Span) -> Self {
        Self { symbol, span }
    }

    pub fn symbol(&self) -> Symbol {
        self.symbol
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn as_str<'a>(&self, interner: &'a Interner) -> &'a str {
        interner.resolve(&self.symbol)
    }

    pub fn as_token(&self) -> TokenKind {
        TokenKind::Ident(*self)
    }

    pub fn is_keyword(&self, interner: &Interner) -> bool {
        let ident_str = interner.resolve(&self.symbol);
        KEYWORDS.contains(&ident_str)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Ident {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        if let Some(token) = stream.peek() {
            if let Some(keyword_ident) = contextual_keyword_ident(token.kind()) {
                stream.advance();
                let span = stream.span();
                let symbol = stream.interner().get_or_intern(keyword_ident);
                return Ok(Ident { symbol, span });
            }
        }

        // Otherwise, parse as a regular identifier
        let ident = consume_token!(stream, TokenKind::Ident(ident) => ident);
        Ok(*ident)
    }
}

fn contextual_keyword_ident(token_kind: &TokenKind) -> Option<&'static str> {
    match token_kind {
        TokenKind::DefaultKw => Some("default"),
        TokenKind::SelfKw => Some("self"),
        TokenKind::SelfType => Some("Self"),
        TokenKind::Super => Some("super"),
        TokenKind::Crate => Some("crate"),
        TokenKind::Pkg => Some("pkg"),
        TokenKind::Start => Some("start"),
        TokenKind::Limit => Some("limit"),
        TokenKind::Asc => Some("asc"),
        TokenKind::Desc => Some("desc"),
        TokenKind::Order => Some("order"),
        TokenKind::RangeKw => Some("range"),
        TokenKind::HopsKw => Some("hops"),
        TokenKind::Enumerate => Some("enumerate"),
        TokenKind::Distinct => Some("distinct"),
        _ => None,
    }
}

impl std::hash::Hash for Ident {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.symbol.hash(state);
        // self.span.hash(state);
    }
}

impl PartialEq for Ident {
    fn eq(&self, other: &Self) -> bool {
        // self.symbol == other.symbol && self.span == other.span
        self.symbol == other.symbol
    }
}

const KEYWORDS: &[&str] = &[
    "select", "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
];

impl From<Ident> for TokenKind {
    fn from(ident: Ident) -> Self {
        TokenKind::Ident(ident)
    }
}

// impl ParseChars for Ident {
//     fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
//         cursor.parse::<LexerIdent>().map(|li| Ident {
//             symbol
//         })
//     }
// }

//
