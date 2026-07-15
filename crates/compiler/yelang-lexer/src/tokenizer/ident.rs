/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 12/02/2025
 */
use super::TokenKind;
use crate::{Interner, Symbol};
use crate::{ParseTokenStream, Span, TokenResult, TokenStream, consume_token};

/// Origin of an identifier token. Used for hygiene special forms such as
/// `$crate` and `$package` inside macro transcribers.
#[derive(Debug, Default, Eq, Clone, Copy, PartialEq, Hash)]
pub enum IdentOrigin {
    /// Ordinary identifier.
    #[default]
    Plain,
    /// `$crate` — resolves to the macro's defining crate root.
    Crate,
    /// `$package` — resolves to the package root.
    Package,
}

#[derive(Debug, Eq, Clone, Copy)]
pub struct Ident {
    pub symbol: Symbol,
    pub span: Span,
    /// Hygiene origin for identifiers produced by macro expansion.
    pub origin: IdentOrigin,
}

impl Ident {
    pub fn new(symbol: Symbol, span: Span) -> Self {
        Self {
            symbol,
            span,
            origin: IdentOrigin::Plain,
        }
    }

    pub fn new_with_origin(symbol: Symbol, span: Span, origin: IdentOrigin) -> Self {
        Self {
            symbol,
            span,
            origin,
        }
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
                return Ok(Ident {
                    symbol,
                    span,
                    origin: IdentOrigin::Plain,
                });
            }
        }

        // `$crate` / `$package` hygiene special forms inside paths.
        if let Some(dollar_crate) = parse_dollar_crate(stream) {
            return Ok(dollar_crate);
        }

        // Otherwise, parse as a regular identifier
        let ident = consume_token!(stream, TokenKind::Ident(ident) => ident);
        Ok(*ident)
    }
}

fn parse_dollar_crate(stream: &mut TokenStream<TokenKind>) -> Option<Ident> {
    // Inspect the next two tokens without mutably borrowing `stream`, then
    // advance once we know this is really `$crate` / `$package`.
    let (first_span, second_span, origin, symbol_text) = {
        let first = stream.peek()?;
        if !matches!(first.kind(), TokenKind::Dollar) {
            return None;
        }
        let second = stream.peek_ahead(1)?;
        let (origin, symbol_text) = match second.kind() {
            TokenKind::Crate => (IdentOrigin::Crate, "crate"),
            TokenKind::Pkg => (IdentOrigin::Package, "pkg"),
            TokenKind::Ident(ident) => {
                let text = stream.interner().resolve(&ident.symbol);
                match text {
                    "crate" => (IdentOrigin::Crate, "crate"),
                    "package" => (IdentOrigin::Package, "package"),
                    _ => return None,
                }
            }
            _ => return None,
        };
        (first.span(), second.span(), origin, symbol_text)
    };

    stream.advance();
    stream.advance();
    let span = first_span.merge(second_span);
    let symbol = stream.interner().get_or_intern(symbol_text);
    Some(Ident {
        symbol,
        span,
        origin,
    })
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
