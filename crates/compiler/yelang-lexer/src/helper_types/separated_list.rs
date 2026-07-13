/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use crate::{
    CharCursor, CharLexerError, CharLexerResult, ParseChars, ParseTokenStream, Span, TokenError,
    TokenResult, TokenStream, TokenTrait,
};
use std::marker::PhantomData;

/// Separated list with at least 1 item
///
/// The `ALLOW_TRAILING` const parameter controls whether trailing separators are consumed:
/// - `false`: Trailing separators are left in the stream (e.g., use statements)
/// - `true`: Trailing separators are consumed (e.g., struct fields, function parameters)
#[derive(Debug, Clone, PartialEq)]
pub struct SeparatedList<TItem, Sep, const ALLOW_TRAILING: bool> {
    parser: Vec<TItem>,
    separator_count: usize,
    separator_marker: PhantomData<Sep>,
    span: Span,
}

impl<P, S, const ALLOW_TRAILING: bool> SeparatedList<P, S, ALLOW_TRAILING> {
    pub fn separator_count(&self) -> usize {
        self.separator_count
    }

    pub fn separator_marker(&self) -> PhantomData<S> {
        self.separator_marker
    }

    pub fn items(self) -> Vec<P> {
        self.parser
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

    pub fn span(&self) -> Span {
        self.span
    }
}

impl<P: ParseChars, S: ParseChars, const ALLOW_TRAILING: bool> ParseChars
    for SeparatedList<P, S, ALLOW_TRAILING>
{
    fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
        let mut items = Vec::new();
        let first_checkpoint = cursor.checkpoint();
        let mut sep_count = 0;

        match cursor.parse::<P>() {
            Ok(first) => items.push(first),
            Err(_) => {
                return Err(CharLexerError::UnexpectedStr {
                    expected: "Separated list".to_string(),
                    found: cursor.peek().unwrap_or_default().to_string(),
                    span: cursor.current_span(),
                });
            }
        }

        loop {
            let checkpoint = cursor.checkpoint();
            match cursor.parse::<S>() {
                Ok(_) => match cursor.parse::<P>() {
                    Ok(item) => {
                        sep_count += 1;
                        items.push(item)
                    }
                    Err(_) => {
                        if ALLOW_TRAILING {
                            // Consume the trailing separator
                            break;
                        } else {
                            // Restore checkpoint to leave the separator unparsed
                            cursor.restore(checkpoint);
                            break;
                        }
                    }
                },
                Err(_) => break,
            }
        }

        Ok(Self {
            parser: items,
            separator_count: sep_count,
            separator_marker: PhantomData,
            span: cursor.span_since(first_checkpoint),
        })
    }
}

impl<PItem, Sep, const ALLOW_TRAILING: bool, TTokenMeta: TokenTrait> ParseTokenStream<TTokenMeta>
    for SeparatedList<PItem, Sep, ALLOW_TRAILING>
where
    PItem: ParseTokenStream<TTokenMeta>,
    Sep: ParseTokenStream<TTokenMeta>,
{
    fn parse(tokenstream: &mut TokenStream<TTokenMeta>) -> TokenResult<Self> {
        let mut items = Vec::new();
        let first_checkpoint = tokenstream.checkpoint();
        let mut sep_count = 0;

        // IMPORTANT:
        // We must not eagerly roll back on item parse failure here.
        // If the item starts parsing and then fails, we want to preserve the
        // underlying error (it is usually much more precise than "expected Separated List").
        let first_item_checkpoint = tokenstream.checkpoint();
        match <PItem as ParseTokenStream<TTokenMeta>>::parse(tokenstream) {
            Ok(first) => items.push(first),
            Err(err) => {
                if tokenstream.slice_since(first_item_checkpoint).is_empty() {
                    tokenstream.restore(first_item_checkpoint);
                    return Err(TokenError::UnexpectedToken {
                        expected: "Separated List".to_string(),
                        found: tokenstream
                            .peek()
                            .map(|t| t.to_string())
                            .unwrap_or_default(),
                        span: tokenstream.current_span(),
                    });
                }

                return Err(err);
            }
        }

        loop {
            let checkpoint = tokenstream.checkpoint();
            if let Ok(_sep) = tokenstream.parse::<Sep>() {
                // Successfully parsed separator, now try to parse item
                let item_checkpoint = tokenstream.checkpoint();
                match <PItem as ParseTokenStream<TTokenMeta>>::parse(tokenstream) {
                    Ok(item) => {
                        // Successfully parsed item after separator
                        items.push(item);
                        sep_count += 1;
                    }
                    Err(err) => {
                        // Parsed separator but no item follows (trailing separator)
                        if tokenstream.slice_since(item_checkpoint).is_empty() {
                            if ALLOW_TRAILING {
                                // Consume the trailing separator
                                break;
                            } else {
                                // Restore checkpoint to leave the separator unparsed
                                tokenstream.restore(checkpoint);
                                break;
                            }
                        }

                        return Err(err);
                    }
                }
            } else {
                // No separator found, we're done
                break;
            }
        }

        // panic!("xxxxxxitem: {:?}", tokenstream.peek());
        Ok(Self {
            parser: items,
            separator_count: sep_count,
            separator_marker: PhantomData,
            span: tokenstream.span_since(first_checkpoint),
        })
    }
}
