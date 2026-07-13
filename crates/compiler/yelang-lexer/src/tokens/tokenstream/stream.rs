use super::super::{ParseTokenStream, TokenError, TokenResult};
use super::{SynMeta, Token, TokenCheckpoint, TokenStreamBuilder, TokenTrait};
use crate::{Interner, Span};
use std::fmt::Display;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TokenStream<Tkind: TokenTrait> {
    pub tokens: Arc<[Token<Tkind>]>,
    position: usize,
    current_span: Span,
    pub interner: Interner,
}

impl<Tkind: TokenTrait> Default for TokenStream<Tkind> {
    fn default() -> Self {
        TokenStream {
            tokens: Arc::from([]),
            position: 0,
            current_span: Span::default(),
            interner: Interner::new(),
        }
    }
}

impl<Tkind: TokenTrait> TokenStream<Tkind> {
    pub(crate) fn new_built(
        tokens: Arc<[Token<Tkind>]>,
        interner: Interner,
        current_span: Span,
    ) -> Self {
        TokenStream {
            tokens,
            position: 0,
            current_span,
            interner,
        }
    }

    pub fn as_tokens(&self) -> Vec<Tkind> {
        self.tokens.iter().map(|t| t.kind().clone()).collect()
    }
}

impl<T: TokenTrait> Display for TokenStream<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for token in self.tokens.iter() {
            write!(f, "{} ", token.kind())?;
        }
        Ok(())
    }
}

impl<TKind> TokenStream<TKind>
where
    TKind: TokenTrait,
{
    pub fn init() -> TokenStreamBuilder<TKind> {
        TokenStreamBuilder::new(Interner::new())
    }

    pub fn new_with_tokens(tokens: Vec<Token<TKind>>, interner: Interner) -> Self {
        TokenStream {
            tokens: tokens.into(),
            position: 0,
            current_span: Span::default(),
            interner,
        }
    }

    pub fn new_from_arc(tokens: Arc<[Token<TKind>]>, interner: Interner) -> Self {
        TokenStream {
            tokens,
            position: 0,
            current_span: Span::default(),
            interner,
        }
    }

    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    pub fn peek(&self) -> Option<&Token<TKind>> {
        self.tokens.get(self.position)
    }

    pub fn peek_behind(&self) -> Option<&Token<TKind>> {
        self.tokens.get(self.position - 1)
    }

    pub fn peek_ahead(&self, n: usize) -> Option<&Token<TKind>> {
        self.tokens.get(self.position + n)
    }

    pub fn advance(&mut self) -> Option<&Token<TKind>> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        // Track only the last consumed token span. Larger construct spans are computed via
        // `checkpoint()` + `span_since()`.
        self.current_span = token.span();
        Some(token)
    }

    pub fn advance_by(&mut self, n: usize) -> TokenResult<&[Token<TKind>]> {
        let checkpoint = self.checkpoint();
        for _ in 0..n {
            if self.advance().is_none() {
                return Err(TokenError::UnexpectedEof {
                    expected: format!("{} tokens", n),
                    span: checkpoint.current_span,
                });
            }
        }

        Ok(self.slice(checkpoint.position, self.position))
    }

    pub fn overall_span(&self) -> Option<Span> {
        if self.tokens.is_empty() {
            None
        } else {
            // Assume tokens are in order
            let first = &self.tokens[0];
            let last = self.tokens.last().unwrap();
            Some(first.span().merge(last.span()))
        }
    }

    /// Advance until the stream reaches the given position.
    pub fn advance_to(&mut self, target: usize) -> TokenResult<&[Token<TKind>]> {
        let checkpoint = self.checkpoint();
        if target > self.tokens.len() {
            return Err(TokenError::UnexpectedEof {
                expected: format!("{} tokens", target),
                span: checkpoint.current_span,
            });
        }
        while self.position < target {
            // Call advance() so that current_span is updated appropriately.
            self.advance();
        }
        Ok(self.slice(checkpoint.position, self.position))
    }

    pub fn advance_until(&mut self, target: &TKind) -> TokenResult<&[Token<TKind>]> {
        let checkpoint = self.checkpoint();
        while let Some(token) = self.peek() {
            if token.kind() == target {
                return Ok(self.slice(checkpoint.position, self.position));
            }
            self.advance();
        }
        Err(TokenError::UnexpectedEof {
            expected: format!("{:?}", target),
            span: self.current_span,
        })
    }

    pub fn advance_until_b4(&mut self, kind: TKind) -> TokenResult<&[Token<TKind>]> {
        let checkpoint = self.checkpoint();
        while let Some(token) = self.peek() {
            if token.kind() == &kind {
                break;
            }
            self.advance();
        }
        Ok(self.slice(checkpoint.position, self.position))
    }

    pub fn until_b4<A, B>(&mut self) -> TokenResult<(Vec<A>, Span)>
    where
        A: ParseTokenStream<TKind>,
        B: ParseTokenStream<TKind>,
    {
        let start = self.checkpoint();
        let mut content_checkpoint = start;
        let mut items = Vec::new();

        loop {
            let checkpoint = self.checkpoint();
            match self.parse::<B>() {
                Ok(_) => {
                    let span = self.span_since(content_checkpoint);
                    return Ok((items, span));
                }
                Err(_) => {
                    self.restore(checkpoint);
                    match self.parse::<A>() {
                        Ok(item) => {
                            content_checkpoint = self.checkpoint();
                            items.push(item)
                        }
                        Err(e) => {
                            // advance to prevent infinite loop, if no progress
                            if self.position == checkpoint.position {
                                self.advance();
                            }
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> &[Token<TKind>] {
        &self.tokens[start..end.min(self.tokens.len())]
    }

    pub fn slice_since(&self, checkpoint: TokenCheckpoint) -> &[Token<TKind>] {
        self.slice(checkpoint.position, self.position)
    }

    pub fn checkpoint(&self) -> TokenCheckpoint {
        // Use the span of the next token to be consumed (if any) as the starting point.
        // This avoids cumulative “span so far” behavior that can cause huge spans and
        // `AstPtr` collisions during resolution.
        let current_span = self.peek().map(|t| t.span()).unwrap_or(self.current_span);
        TokenCheckpoint {
            position: self.position,
            current_span,
        }
    }

    pub fn restore(&mut self, checkpoint: TokenCheckpoint) {
        self.position = checkpoint.position;

        self.current_span = checkpoint.current_span;
    }

    pub fn span(&self) -> Span {
        self.current_span
    }

    pub fn current_span(&self) -> Span {
        self.current_span
    }

    pub fn is_eof(&self) -> bool {
        self.position >= self.tokens.len()
    }

    pub fn consume_exact<F>(
        &mut self,
        count: usize,
        predicate: F,
    ) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        self.consume_while_base(predicate, Some(count), Some(count))
    }

    pub fn consume(&mut self, expected: impl Into<TKind>) -> Result<&Token<TKind>, TokenError> {
        let expected = expected.into();
        let token = self.peek();
        let current_span = self.current_span;

        match token {
            Some(t) if t.kind() == &expected => {
                self.advance().ok_or_else(|| TokenError::UnexpectedEof {
                    expected: format!("{:?}", expected),
                    span: current_span,
                })
            }
            Some(t) => Err(TokenError::UnexpectedToken {
                expected: format!("{:?}", expected),
                found: format!("{:?}", t.kind()),
                span: t.span(),
            }),
            None => Err(TokenError::UnexpectedEof {
                expected: format!("{:?}", expected),
                span: current_span,
            }),
        }
    }

    pub fn consume_with_meta(
        &mut self,
        expected: impl Into<TKind>,
    ) -> Result<&Token<TKind>, TokenError> {
        let expected = expected.into();
        let token = self.peek();
        let current_span = self.current_span;
        match token {
            Some(t) if t.kind() == &expected => {
                self.advance().ok_or_else(|| TokenError::UnexpectedEof {
                    expected: format!("{:?}", expected),
                    span: current_span,
                })
            }
            Some(t) => Err(TokenError::UnexpectedToken {
                expected: format!("{:?}", expected),
                found: format!("{:?}", t.kind()),
                span: t.span(),
            }),
            None => Err(TokenError::UnexpectedEof {
                expected: format!("{:?}", expected),
                span: current_span,
            }),
        }
    }

    pub fn consume_token_fn<F>(&mut self, predicate: F) -> Result<&Token<TKind>, TokenError>
    where
        F: Fn(&Token<TKind>) -> bool,
    {
        self.consume_exact(1, predicate).map(|tokens| &tokens[0])
    }

    pub fn consume_map<F, U>(&mut self, f: F) -> Result<U, TokenError>
    where
        F: FnOnce(&Token<TKind>) -> Option<U>,
    {
        let token = self.peek().ok_or_else(|| TokenError::UnexpectedEof {
            expected: "some token".to_string(),
            span: self.current_span,
        })?;

        match f(token) {
            Some(value) => {
                let _ = self.advance();
                Ok(value)
            }
            None => Err(TokenError::UnexpectedToken {
                expected: "custom pattern".to_string(),
                found: format!("{:?}", token.kind()),
                span: token.span(),
            }),
        }
    }

    pub fn current(&self) -> Option<&Token<TKind>> {
        self.tokens.get(self.position)
    }

    pub fn consume_until<F>(&mut self, mut predicate: F) -> &[Token<TKind>]
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        let start_pos = self.position;
        while !self.is_eof() {
            let current = &self.tokens[self.position];
            if predicate(current) {
                break;
            }
            self.advance();
        }
        &self.tokens[start_pos..self.position]
    }

    pub fn verify(&mut self, expected: TKind) -> Result<Span, TokenError> {
        let start = self.checkpoint();
        let result = match self.consume(expected) {
            Ok(_) => {
                let span = self.span_since(start);
                Ok(span)
            }
            Err(e) => Err(e),
        };
        self.restore(start);
        result
    }

    pub fn verify_type<U: ParseTokenStream<TKind>>(&mut self) -> Result<Span, TokenError> {
        let start = self.checkpoint();
        self.parse::<U>()?;
        let span = self.span_since(start);
        self.restore(start);
        Ok(span)
    }

    pub fn verify_exact(
        &mut self,
        count: usize,
        predicate: impl Fn(&Token<TKind>) -> bool,
    ) -> bool {
        let start = self.checkpoint();
        let mut found = 0;
        while let Some(token) = self.peek() {
            if predicate(token) {
                found += 1;
                if found == count {
                    self.restore(start);
                    return true;
                }
            }
            self.advance();
        }
        self.restore(start);
        false
    }

    pub fn remaining(&self) -> Vec<Token<TKind>> {
        self.tokens[self.position..].to_vec()
    }

    pub fn reset_dangerous(&mut self) {
        self.position = 0;
        self.current_span = Span::default();
    }

    pub fn parse_with<F, R>(&mut self, mut parser: F) -> Result<(R, Span), TokenError>
    where
        F: FnMut(&mut Self) -> Result<R, TokenError>,
    {
        let start = self.checkpoint();
        let value = parser(self)?;
        let span = start.current_span.merge(self.current_span);
        Ok((value, span))
    }

    // TODO:  rethink naming: Should I name this consume_type
    pub fn parse<TInput: ParseTokenStream<TKind>>(&mut self) -> TokenResult<TInput> {
        let checkpoint = self.checkpoint();
        TInput::parse(self).inspect_err(|_| {
            self.restore(checkpoint);
        })
    }

    pub fn verify_parse<TInput: ParseTokenStream<TKind>>(&mut self) -> Result<Span, TokenError> {
        let checkpoint = self.checkpoint();
        let _ = TInput::parse(self)?;
        let span = self.span_since(checkpoint);
        self.restore(checkpoint);
        Ok(span)
    }

    pub fn parse_map<TInput, TOutput>(&mut self) -> TokenResult<TOutput>
    where
        TInput: ParseTokenStream<TKind, TOutput>,
    {
        let checkpoint = self.checkpoint();
        TInput::parse(self).inspect_err(|_| {
            self.restore(checkpoint);
        })
    }

    /// NOTE: Still an experimental feature. Might be removed in the future. Not recommended for use.
    /// and nto even sure it's useful but science is about breaking rules not laws.
    /// True Laws cannot/shouldn't be broken because they are independent of the observer.
    /// Rules are meant to be broken because they are dependent on the observer.
    /// Think like there is no box. Lead with Love and respect for humanity and nature and the
    /// unborn generation. Love is the ultimate civilization. Break the rules cos that's how
    /// science advances, respect the laws cos that's how humanity advances.
    /// let x = tokens.parse_to::<Word<'L', 'L'>,  TokenMeta>().unwrap();
    pub fn parse_to<U, T>(&mut self) -> TokenResult<T>
    where
        U: ParseTokenStream<TKind, T>,
    {
        let checkpoint = self.checkpoint();
        U::parse(self).inspect_err(|_| {
            self.restore(checkpoint);
        })
    }

    pub fn parse_many<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<Vec<U>> {
        let mut values = Vec::new();
        while !self.is_eof() {
            values.push(self.parse()?);
        }
        Ok(values)
    }

    pub fn span_since(&self, checkpoint: TokenCheckpoint) -> Span {
        checkpoint.current_span.merge(self.current_span)
    }

    pub fn parse_with_span<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<(U, Span)> {
        let checkpoint = self.checkpoint();
        let value = self.parse::<U>()?;
        let span = self.span_since(checkpoint);
        Ok((value, span))
    }

    pub fn parse_with_meta<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<SynMeta<U>> {
        let checkpoint = self.checkpoint();
        let value = self.parse::<U>()?;
        let span = self.span_since(checkpoint);
        Ok(SynMeta::new(value, span))
    }

    pub fn parse_exact<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<U> {
        let value = self.parse::<U>()?;
        if !self.is_eof() {
            let token = self.peek().expect(
                "EOF check failed. This is a bug. Please report at github.com/oyelowo/yedb.",
            );
            return Err(TokenError::UnexpectedToken {
                expected: "EOF".to_string(),
                found: format!("{:?}", token.kind()),
                span: token.span(),
            });
        }
        Ok(value)
    }

    pub fn parse_exact_with_span<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<(U, Span)> {
        let checkpoint = self.checkpoint();
        let value = self.parse_exact::<U>()?;
        let span = self.span_since(checkpoint);
        Ok((value, span))
    }

    pub fn parse_many_exact<U: ParseTokenStream<TKind>>(&mut self) -> TokenResult<Vec<U>> {
        let mut values = Vec::new();
        while !self.is_eof() {
            values.push(self.parse_exact::<U>()?);
        }
        Ok(values)
    }

    pub fn optional<U>(&mut self) -> TokenResult<Option<U>>
    where
        U: ParseTokenStream<TKind>,
    {
        let checkpoint = self.checkpoint();
        match self.parse::<U>() {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                self.restore(checkpoint);
                Ok(None)
            }
        }
    }

    fn consume_while_base<F>(
        &mut self,
        mut predicate: F,
        min_tokens: Option<usize>,
        max_tokens: Option<usize>,
    ) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        let checkpoint = self.checkpoint();
        let start_pos = self.position;
        let mut count = 0;

        while !self.is_eof() {
            if max_tokens.is_some_and(|max| count >= max) {
                break;
            }

            let token = &self.tokens[self.position];
            if !predicate(token) {
                break;
            }

            self.advance();
            count += 1;
        }

        if let Some(min) = min_tokens {
            if count < min {
                self.restore(checkpoint);
                return Err(TokenError::CustomError {
                    msg: format!(
                        "Needed at least {min} tokens, but found {count} while consuming tokens"
                    ),
                    span: self.current_span,
                });
            }
        }

        Ok(&self.slice(start_pos, start_pos + count))
    }

    pub fn consume_while<F>(&mut self, predicate: F) -> &[Token<TKind>]
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        self.consume_while_base(predicate, None, None)
            .unwrap_or_default()
    }

    pub fn consume_while_m_n<F>(
        &mut self,
        min_tokens: usize,
        max_tokens: usize,
        predicate: F,
    ) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        self.consume_while_base(predicate, Some(min_tokens), Some(max_tokens))
    }

    pub fn consume_while_m<F>(
        &mut self,
        min_tokens: usize,
        predicate: F,
    ) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        self.consume_while_base(predicate, Some(min_tokens), None)
    }

    pub fn consume_while_m_span<F>(
        &mut self,
        min_tokens: usize,
        predicate: F,
    ) -> Result<(&[Token<TKind>], Span), TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        let start = self.checkpoint();
        let start_pos = self.position;

        let count = self.consume_while_m(min_tokens, predicate)?.len();
        let span = start.current_span.merge(self.current_span);
        let consumed = &self.slice(start_pos, start_pos + count);
        Ok((consumed, span))
    }

    pub fn verify_while<F>(&mut self, predicate: F) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        let checkpoint = self.checkpoint();
        let consumed = self.consume_while(predicate);
        let count = consumed.len();
        self.restore(checkpoint);
        Ok(&self.tokens[self.position..self.position + count])
    }

    pub fn verify_while_m<F>(
        &mut self,
        min_tokens: usize,
        predicate: F,
    ) -> Result<&[Token<TKind>], TokenError>
    where
        F: FnMut(&Token<TKind>) -> bool,
    {
        let checkpoint = self.checkpoint();
        let consumed = self.consume_while_m(min_tokens, predicate)?;
        let count = consumed.len();
        self.restore(checkpoint);
        Ok(&self.tokens[self.position..self.position + count])
    }

    pub fn verify_while_m_n(
        &mut self,
        min_tokens: usize,
        max_tokens: usize,
        predicate: impl FnMut(&Token<TKind>) -> bool,
    ) -> Result<&[Token<TKind>], TokenError> {
        let checkpoint = self.checkpoint();
        let consumed = self.consume_while_m_n(min_tokens, max_tokens, predicate)?;
        let count = consumed.len();
        self.restore(checkpoint);
        Ok(&self.tokens[self.position..self.position + count])
    }
}
