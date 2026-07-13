use std::fmt::Display;

use crate::{Char, TokenStream, TokenTrait};

use crate::{Either, ParseChars, Repeat};

use super::{
    Boolean, CharCursor, CharLexerError, Comment, DatetimeLexed, DurationLexed, Float, Geometry,
    Ident, IntSigned, Keyword, RecordIdLexed, Span, StringLit, UuidLexed, Whitespace,
    str_lit::{Modifiers, StringTag},
};
use crate::utils::{Interner, Symbol};

/// Primary macro that captures the initial checkpoint.
#[macro_export]
macro_rules! token_mapper {
    ($cursor:expr, $($attempt:expr),+ $(,)?) => {{
         let __checkpoint = $cursor.checkpoint();
         token_mapper_inner!($cursor, __checkpoint, $($attempt),+)
    }};
}

/// Helper macro that recursively chains the mapping attempts.
#[macro_export]
macro_rules! token_mapper_inner {
    ($cursor:expr, $checkpoint:expr, $first:expr, $($rest:expr),+ $(,)?) => {{
         $first.or_else(|_| {
              // Restore to the same checkpoint before the next attempt.
              $cursor.restore($checkpoint);
              token_mapper_inner!($cursor, $checkpoint, $($rest),+)
         })
    }};
    ($cursor:expr, $checkpoint:expr, $attempt:expr $(,)?) => {
         $attempt
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaggedValue<T> {
    pub content: T,
    pub tag: StringTag,
    pub modifiers: Option<Modifiers>,
    pub delimiter_level: u8,
    pub span: Span,
    pub quote_char: char,
}

impl<T: Display> Display for TaggedValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Keyword(Keyword),
    Ident(InternedIdent),
    Boolean(Boolean),
    StringLit(InternedStringLit),
    Symbol(Symbol),
    Float(Float),
    Comment(Comment), // TODO: intern?
    Int(IntSigned),
    Datetime(TaggedValue<DatetimeLexed>),
    Duration(TaggedValue<DurationLexed>),
    Geometry(TaggedValue<Geometry>),
    Uuid(TaggedValue<UuidLexed>),
    RecordId(TaggedValue<RecordIdLexed>),
    Mana,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InternedIdent {
    pub symbol: Symbol,
    pub span: Span,
    pub is_raw: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InternedStringLit {
    pub symbol: Symbol,
    pub tag: Option<StringTag>,
    pub modifiers: Option<Modifiers>,
    pub delimiter_level: u8,
    pub span: Span,
    pub quote_char: char,
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenKind::Keyword(k) => write!(f, "{k}"),
            TokenKind::Ident(s) => write!(f, "{}", s.symbol.0), // TODO: resolve
            TokenKind::StringLit(s) => write!(f, "{}{}{}", s.quote_char, s.symbol.0, s.quote_char), // TODO
            TokenKind::Boolean(b) => write!(f, "{b}"),
            TokenKind::Symbol(s) => write!(f, "{s}"),
            TokenKind::Float(s) => write!(f, "{s}"),
            TokenKind::Comment(s) => write!(f, "{s:?}"),
            TokenKind::Int(s) => write!(f, "{s}"),
            TokenKind::Datetime(d) => write!(f, "{d}"),
            TokenKind::Duration(d) => write!(f, "{d}"),
            TokenKind::Geometry(g) => write!(f, "{g}"),
            TokenKind::Uuid(u) => write!(f, "{u}"),
            TokenKind::RecordId(r) => write!(f, "{r}"),
            TokenKind::Mana => write!(f, "mana"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenMeta {
    pub kind: TokenKind,
    pub span: Span,
}

impl TokenTrait for TokenMeta {
    type Kind = TokenKind;

    fn kind(&self) -> &Self::Kind {
        &self.kind
    }

    fn span(&self) -> Span {
        self.span
    }
}

pub struct CharLexer<'a> {
    input: &'a str,
    interner: &'a mut Interner,
}

impl<'a> CharLexer<'a> {
    pub fn new(input: &'a str, interner: &'a mut Interner) -> Self {
        Self { input, interner }
    }

    pub fn tokenize(&self) -> Result<TokenStream<TokenMeta>, CharLexerError> {
        let mut cursor = CharCursor::new(self.input);
        let mut tokens = Vec::new();

        while let Some(token) = Self::next_token(&mut cursor, self.interner)? {
            tokens.push(token);
        }

        Ok(TokenStream::new(tokens))
    }

    /// A shared utility to parse the content of a tagged string literal.
    /// It ensures the specific parser consumes the entire content, preventing partial matches.
    fn parse_tagged_content<T: ParseChars>(
        cursor: &CharCursor<'a>,
        s: &StringLit,
        tag: StringTag,
        error_msg: &'static str,
    ) -> Result<TaggedValue<T>, CharLexerError> {
        let content_str = cursor.str_from_span(s.raw_content);
        let mut sub_cursor = CharCursor::new(content_str);

        let content = T::parse(&mut sub_cursor).map_err(|_| CharLexerError::InvalidIdent {
            found: error_msg.to_string(),
            span: s.raw_content,
        })?;

        if !sub_cursor.is_eof() {
            return Err(CharLexerError::InvalidIdent {
                found: format!("{}: unexpected trailing characters", error_msg),
                span: s.raw_content,
            });
        }

        Ok(TaggedValue {
            content,
            tag,
            modifiers: s.modifiers,
            delimiter_level: s.delimiter_level,
            span: s.span,
            quote_char: s.quote_char,
        })
    }

    fn next_token(
        cursor: &mut CharCursor<'a>,
        interner: &mut Interner,
    ) -> Result<Option<TokenMeta>, CharLexerError> {
        cursor.parse::<Repeat<Either<Comment, Whitespace>>>().ok();
        if cursor.peek().is_none() {
            return Ok(None);
        }

        let start_pos = cursor.checkpoint();

        let token_kind = token_mapper!(
            cursor,
            cursor.parse::<StringLit>().and_then(|s| {
                if let Some(tag) = s.tag {
                    let tag_str = cursor.str_from_span(tag.value);
                    match tag_str {
                        "dt" => {
                            Self::parse_tagged_content(cursor, &s, tag, "Invalid datetime content")
                                .map(TokenKind::Datetime)
                        }
                        "dur" => {
                            Self::parse_tagged_content(cursor, &s, tag, "Invalid duration content")
                                .map(TokenKind::Duration)
                        }
                        "geo" => {
                            Self::parse_tagged_content(cursor, &s, tag, "Invalid geometry content")
                                .map(TokenKind::Geometry)
                        }
                        "uuid" => {
                            Self::parse_tagged_content(cursor, &s, tag, "Invalid uuid content")
                                .map(TokenKind::Uuid)
                        }
                        "rid" => {
                            Self::parse_tagged_content(cursor, &s, tag, "Invalid record id content")
                                .map(TokenKind::RecordId)
                        }
                        _ => {
                            // unknown tag, treat as a regular string
                            let content = cursor.str_from_span(s.raw_content);
                            let symbol = interner.intern(content);
                            Ok(TokenKind::StringLit(InternedStringLit {
                                symbol,
                                tag: Some(tag),
                                modifiers: s.modifiers,
                                delimiter_level: s.delimiter_level,
                                span: s.span,
                                quote_char: s.quote_char,
                            }))
                        }
                    }
                } else {
                    // no tag, regular string
                    let content = cursor.str_from_span(s.raw_content);
                    let symbol = interner.intern(content);
                    Ok(TokenKind::StringLit(InternedStringLit {
                        symbol,
                        tag: None,
                        modifiers: s.modifiers,
                        delimiter_level: s.delimiter_level,
                        span: s.span,
                        quote_char: s.quote_char,
                    }))
                }
            }),
            // Keywords and booleans before general identifiers
            cursor.parse::<Keyword>().map(TokenKind::Keyword),
            cursor.parse::<Boolean>().map(TokenKind::Boolean),
            // General identifier
            cursor.parse::<Ident>().map(|ident| {
                let value = cursor.str_from_span(ident.span);
                let symbol = interner.intern(value);
                TokenKind::Ident(InternedIdent {
                    symbol,
                    span: ident.span,
                    is_raw: ident.is_raw,
                })
            }),
            cursor.parse::<Symbol>().map(TokenKind::Symbol),
            cursor.parse::<Float>().map(TokenKind::Float),
            cursor.parse::<IntSigned>().map(TokenKind::Int),
            cursor
                .parse_as_str::<(Char<'a'>, Char<'a'>)>()
                .map(|_s| TokenKind::Mana),
        )?;

        Ok(Some(TokenMeta {
            kind: token_kind,
            span: cursor.span_since(start_pos),
        }))
    }
}

#[cfg(test)]
mod lexer_test {
    use crate::OneOf3;

    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::with_space(
        r#"
SELECT {
  user_id: user.id,
  name,
  purchased_products: product[*].{
    product_name: product.name,
    buyers: buyer[*].{
      buyer_name: buyer.name,
      purchase_date: purchased_by.created_at,
      value: '😄',
      value2:   ########''#😄oyelowo111rerex

'#'######
      er
      ooorkkkxx'######## + 94.5e+23
    }
  }
}
FROM user:User
LINKS
  (user) -> [purchased:UserPurchasedProduct] -> (product:Product)
          <- [purchased_by:UserPurchasedProduct] <- (buyer:User);

    "#,
        "geo"
    )]
    // #[case::with_space(r#"let
    // SELECT/* /*nested comment*/ /* hty/*keep nesting */again*/posts: */*from user where name = 'Oyelowo';"#, "geo")]
    // #[case::with_space("geo ", "geo")]
    // #[case::with_newline("geo\n", "geo")]
    // #[case::with_comment("geo//comment", "geo")]
    // #[case::with_comment_and_space("geo //comment", "geo")]
    // #[case::with_comment_and_newline("geo\n//comment", "geo")]
    // #[case::with_comment_space_and_newline("geo \n//comment", "geo")]
    // #[case::with_comment_space_and_newline_2("geo \n //comment", "geo")]
    // #[case::with_comment_space_and_newline_3("geo \n //comment\n", "geo")]
    // #[case::with_comment_space_and_newline_4("geo \n //comment\n ", "geo")]
    // #[case::with_comment_space_and_newline_5("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_6("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_7("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_8("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_9("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_10("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_11("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_12("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_13("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_14("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_15("geo \n //comment\n  ", "geo")]
    // #[case::with_comment_space_and_newline_16("geo \n //comment\n  ", "geo")]
    fn test_lexer(#[case] input: &str, #[case] expected: &str) {
        use crate::{Any, Eof};

        let mut interner = Interner::new();
        let lexer = CharLexer::new(input, &mut interner);
        let mut tokens = lexer.tokenize().unwrap();
        // panic!("yyy{:#?}", tokens.peek());
        // tokens.parse(TokenKind::Keyword(Keyword::From)).unwrap();
        // tokens.consume(TokenKind::Keyword(Keyword::Return)).unwrap();
        // tokens.parse::<(OneOf3<T!["`"], Where, T!["*"]>, T!["{"])>().map_err(|e| {
        //     panic!("xxx{:#}", e);
        // }).unwrap();
        panic!("xxx{:#?}", tokens.tokens);
        // panic!("yyy{:#?}", tokens.peek());
        // debug_assert_eq!(tokens, );
        // assert_eq!(tokens, expected);
        // assert_eq!(tokens.to_string(), expected);
        // let token = tokens.into_iter().next().unwrap();
        // assert_eq!(token.kind.to_string(), expected);
    }
}
