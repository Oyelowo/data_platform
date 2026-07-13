use super::{Token, TokenStream, TokenStreamBuilder, TokenTrait};
use crate::Interner;
use crate::{CharCursor, CharLexerResult};

/// A trait for tokenizing input strings into a stream of tokens.
///
/// By default, it provides:
/// - A method (`create_cursor`) to initialize a `CharCursor` for traversing the string.
/// - A `tokenize` method that repeatedly calls `next_token` until no more tokens are found.
/// - A required `next_token` method for extracting the next token from the cursor.
///
/// Example usage:
///
/// ```rust
/// use crate::{TokenizeChars, CharLexerResult, CharLexerError, CharCursor, TokenMeta, TokenStream};
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum TokenKind<'a> {
///    Ident(&'a str),
///    Number(i32),
///    Plus,
///    Minus,
///    Star,
/// }
///
/// impl<'input> TokenizeChars<'input> for TokenKind<'input> {
///     fn next_token(cursor: &mut CharCursor<'input>) -> CharLexerResult<Option<Token<Self>>> {
///      cursor.parse::<Repeat<Either<Comment, Whitespace>>>().ok();
///
///      if cursor.peek().is_none() {
///         return Ok(None);
///      }
///
///      let start_pos = cursor.checkpoint();
///
///      let token_kind = token_mapper!(
///      cursor,
///      cursor.parse::<StringLit>().map(Token::StringLit),
///      cursor
///      .parse::<Word6<'S', 'E', 'L', 'E', 'C', 'T'>>()
///      .map(|_| Token::Select),
///      cursor.parse::<Ident>().map(Token::Ident),
///      cursor.parse::<Boolean>().map(Token::Boolean),
///      cursor.parse::<Float>().map(Token::Float),
///      cursor.parse::<Int>().map(Token::Int),
///      cursor
///      .parse::<Word2<'=', '='>>()
///      .map(|_| Token::EqualEqual),
///      cursor.parse::<Char<'='>>().map(|_| Token::Equal),
///      cursor.consume("{").map(|_| Token::OpenBrace),
///      cursor.parse::<Char<'}'>>().map(|_| Token::CloseBrace),
///      cursor.parse::<Char<'+'>>().map(|_| Token::Plus),
///      cursor.parse::<Char<'.'>>().map(|_| Token::Dot),
///      cursor.parse::<Any>().map(|_| Token::Unknown),
///      )?;
///
///      Ok(Some(TokenMeta {
///         kind: token_kind,
///         span: cursor.span_since(start_pos),
///      }))
///     }
/// }
///
pub trait TokenizeChars<'input>
where
    Self: TokenTrait + 'input,
{
    /// Creates a new `CharCursor` from the provided input string.
    #[inline]
    fn create_cursor(input: &'input str) -> CharCursor<'input> {
        CharCursor::new(input)
    }

    /// Tokenizes the entire input string, collecting tokens into a `TokenStream<Self>`.
    ///
    /// It repeatedly calls `next_token` until no token is returned (`Ok(None)`),
    /// then returns the accumulated tokens.

    /// # Example
    /// ```rust, ignore
    /// use crate::{TokenizeChars, CharLexerResult, CharLexerError, CharCursor, TokenMeta, TokenStream};
    ///
    /// #[derive(Debug, Clone, PartialEq)]
    /// enum TokenKind<'a> {
    ///   Ident(&'a str),
    ///   Plus,
    ///   Minus,
    ///   ...
    /// }
    ///
    /// let mut tokens = TokenKind::tokenize(input).unwrap();
    /// ```
    fn tokenize(input: &'input str) -> CharLexerResult<TokenStream<Self>> {
        let mut cursor = Self::create_cursor(input);
        let mut tokens = TokenStreamBuilder::new(Interner::new());

        // Keep extracting tokens until `next_token` returns `None`.
        while let Some(token_meta) = Self::next_token(&mut cursor)? {
            let (kind, span) = token_meta.into_parts();
            tokens.append(kind, span);
        }

        Ok(tokens.build())
    }

    /// Extracts the next token from the cursor (if any).
    ///
    /// - Returns `Ok(None)` when there are no more tokens to parse.
    /// - Returns `Ok(Some(TokenMeta { kind, span }))` for a recognized token.
    /// - Returns an error (`Err(...)`) if parsing fails irrecoverably.
    fn next_token(cursor: &mut CharCursor<'input>) -> CharLexerResult<Option<Token<Self>>>;
}
