/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, CharLexerResult, ParseChars, ParseTokenStream, TokenError,
    TokenResult, TokenStream, TokenTrait,
};

pub struct Repeat<T> {
    parser: Vec<T>,
    count: usize,
}

impl<T> Repeat<T> {
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn value(&self) -> &Vec<T> {
        &self.parser
    }

    pub fn value_mut(&mut self) -> &mut Vec<T> {
        &mut self.parser
    }

    pub fn value_owned(self) -> Vec<T> {
        self.parser
    }
}

impl<T> ParseChars for Repeat<T>
where
    T: ParseChars,
{
    fn parse(cursor: &mut CharCursor) -> Result<Self, CharLexerError> {
        let mut results = Vec::new();
        let mut count = 0;

        loop {
            let checkpoint = cursor.checkpoint();
            match cursor.parse::<T>() {
                Ok(item) => {
                    let after = cursor.checkpoint();
                    if checkpoint == after {
                        // Zero-width match, break to prevent infinite loop
                        cursor.restore(checkpoint);
                        break;
                    }
                    results.push(item);
                    count += 1;
                }
                Err(_) => {
                    cursor.restore(checkpoint);
                    break;
                }
            }
        }

        Ok(Self {
            parser: results,
            count,
        })
    }
}

impl<T, TT: TokenTrait> ParseTokenStream<TT> for Repeat<T>
where
    T: ParseTokenStream<TT>,
{
    fn parse(tokenstream: &mut TokenStream<TT>) -> TokenResult<Self> {
        let mut results = Vec::new();
        let mut count = 0;

        loop {
            let checkpoint = tokenstream.checkpoint();
            match tokenstream.parse::<T>() {
                Ok(item) => {
                    let after = tokenstream.checkpoint();
                    if checkpoint == after {
                        // Zero-width match, break to prevent infinite loop
                        tokenstream.restore(checkpoint);
                        break;
                    }
                    results.push(item);
                    count += 1;
                }
                Err(_) => {
                    // CRITICAL: Restore checkpoint on failure!
                    tokenstream.restore(checkpoint);
                    break;
                }
            }
        }

        Ok(Self {
            parser: results,
            count,
        })
    }
}

// parse min max const time
// example usage but should be generic:
// cursor.parse::<RepeatMinMax<1, 3, Star>>();
#[derive(Debug, Clone)]
pub struct RepeatMinMax<const MIN: usize, const MAX: usize, P> {
    parser: Vec<P>,
}

impl<const MIN: usize, const MAX: usize, P> RepeatMinMax<MIN, MAX, P> {
    pub fn new(parser: Vec<P>) -> Self {
        Self { parser }
    }

    pub fn value(&self) -> &Vec<P> {
        &self.parser
    }

    pub fn value_mut(&mut self) -> &mut Vec<P> {
        &mut self.parser
    }

    pub fn value_owned(self) -> Vec<P> {
        self.parser
    }

    pub fn min(&self) -> usize {
        MIN
    }

    pub fn max(&self) -> usize {
        MAX
    }
}

impl<P: ParseChars, const MIN: usize, const MAX: usize> ParseChars for RepeatMinMax<MIN, MAX, P> {
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let mut results = if MAX < usize::MAX {
            Vec::with_capacity(MAX)
        } else {
            Vec::new()
        };

        let start_total = cursor.checkpoint();

        while results.len() < MAX {
            // 1. Checkpoint before attempting this specific iteration
            let iter_start = cursor.checkpoint();

            match cursor.parse::<P>() {
                Ok(item) => {
                    results.push(item);

                    // 2. Checkpoint after success
                    let iter_end = cursor.checkpoint();

                    // 3. CRITICAL FIX: If we didn't consume anything, break.
                    // Otherwise, we will loop forever matching empty strings.
                    if iter_start == iter_end {
                        break;
                    }
                }
                Err(_) => break,
            }
        }

        if results.len() < MIN {
            Err(CharLexerError::InsufficientRepetition {
                expected: MIN,
                found: results.len(),
                span: cursor.span_since(start_total),
            })
        } else {
            Ok(Self { parser: results })
        }
    }
}

impl<P, const MIN: usize, const MAX: usize, T> ParseTokenStream<T> for RepeatMinMax<MIN, MAX, P>
where
    T: TokenTrait,
    P: ParseTokenStream<T>,
{
    fn parse(tokenstream: &mut TokenStream<T>) -> TokenResult<Self> {
        let mut results = if MAX < usize::MAX {
            Vec::with_capacity(MAX)
        } else {
            Vec::new()
        };

        let start_total = tokenstream.checkpoint();

        while results.len() < MAX {
            // 1. Checkpoint before attempting this specific iteration
            let iter_start = tokenstream.checkpoint();

            match tokenstream.parse::<P>() {
                Ok(item) => {
                    results.push(item);

                    // 2. Checkpoint after success
                    let iter_end = tokenstream.checkpoint();

                    // 3. CRITICAL FIX: If the parser P succeeded but consumed 0 tokens,
                    // we must stop, or we will fill memory infinitely.
                    if iter_start == iter_end {
                        break;
                    }
                }
                Err(_) => {
                    // CRITICAL: Restore checkpoint on failure!
                    tokenstream.restore(iter_start);
                    break;
                }
            }
        }

        if results.len() < MIN {
            return Err(TokenError::InsufficientRepetition {
                expected: MIN,
                found: results.len(),
                span: tokenstream.span_since(start_total),
            });
        }

        Ok(Self { parser: results })
    }
}

pub type RepeatMin<const MIN: usize, P> = RepeatMinMax<MIN, { usize::MAX }, P>;

pub type RepeatMax<const MAX: usize, P> = RepeatMinMax<0, MAX, P>;
pub type RepeatExact<const N: usize, P> = RepeatMinMax<N, N, P>;

pub struct RepeatMinMaxSep<const MIN: usize, const MAX: usize, P, S> {
    parser: Vec<P>,
    separator: Vec<S>,
}

impl<const MIN: usize, const MAX: usize, P, S> RepeatMinMaxSep<MIN, MAX, P, S> {
    pub fn new(parser: Vec<P>, separator: Vec<S>) -> Self {
        Self { parser, separator }
    }

    pub fn value(&self) -> &Vec<P> {
        &self.parser
    }

    pub fn value_mut(&mut self) -> &mut Vec<P> {
        &mut self.parser
    }

    pub fn value_owned(self) -> Vec<P> {
        self.parser
    }

    pub fn min(&self) -> usize {
        MIN
    }

    pub fn max(&self) -> usize {
        MAX
    }

    pub fn separator(&self) -> &Vec<S> {
        &self.separator
    }

    pub fn separator_mut(&mut self) -> &mut Vec<S> {
        &mut self.separator
    }

    pub fn separator_owned(self) -> Vec<S> {
        self.separator
    }
}

impl<P: ParseChars, S: ParseChars, const MIN: usize, const MAX: usize> ParseChars
    for RepeatMinMaxSep<MIN, MAX, P, S>
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let mut results = if MAX < usize::MAX {
            Vec::with_capacity(MAX)
        } else {
            Vec::new()
        };
        let mut separator = Vec::new();
        let start = cursor.checkpoint();

        while results.len() < MAX {
            let iter_start = cursor.checkpoint();
            match cursor.parse::<P>() {
                Ok(item) => {
                    results.push(item);
                    match cursor.parse::<S>() {
                        Ok(sep) => {
                            separator.push(sep);
                            let iter_end = cursor.checkpoint();
                            if iter_start == iter_end {
                                break;
                            }
                        }
                        Err(_) => {
                            // If separator fails, we have consumed P but not S, so we need to restore
                            // to before P, because the P shouldn't be included if separator fails
                            cursor.restore(iter_start);
                            results.pop();
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        if results.len() < MIN {
            Err(CharLexerError::InsufficientRepetition {
                expected: MIN,
                found: results.len(),
                span: cursor.span_since(start),
            })
        } else {
            Ok(Self {
                parser: results,
                separator,
            })
        }
    }
}

struct RepeatUntil<const N: usize, P, U> {
    parser: Vec<P>,
    until: U,
}
