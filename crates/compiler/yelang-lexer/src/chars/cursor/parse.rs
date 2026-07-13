/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

use crate::chars::{CharLexerError, ParseChars};

use super::core::CharCursor;
use super::types::{Checkpoint, Span};

impl<'a> CharCursor<'a> {
    pub fn parse<T: ParseChars>(&mut self) -> Result<T, CharLexerError> {
        let checkpoint = self.checkpoint();
        match T::parse(self) {
            Ok(t) => Ok(t),
            Err(e) => {
                self.restore(checkpoint);
                Err(e)
            }
        }
    }

    pub fn parse_as_str<T: ParseChars>(&mut self) -> Result<&'a str, CharLexerError> {
        let checkpoint = self.checkpoint();
        self.parse::<T>()?;
        Ok(self.slice(checkpoint.current_pos(), self.current_pos))
    }

    /// Execute parser with automatic span tracking
    pub fn parse_with_span<T: ParseChars>(&mut self) -> Result<(T, Span), CharLexerError> {
        let checkpoint = self.checkpoint();
        let value = self.parse::<T>()?;
        Ok((value, self.span_since(checkpoint)))
    }

    pub fn parse_with_span_as_str<T: ParseChars>(
        &mut self,
    ) -> Result<(&'a str, Span), CharLexerError> {
        let checkpoint = self.checkpoint();
        let _value = self.parse::<T>()?;
        Ok((
            self.slice(checkpoint.current_pos(), self.current_pos),
            self.span_since(checkpoint),
        ))
    }

    pub fn parse_exact<T: ParseChars>(&mut self) -> Result<T, CharLexerError> {
        let res = self.parse::<T>();

        if !self.is_eof() {
            return Err(CharLexerError::UnexpectedChar {
                expected: "end of input".to_string(),
                found: self.peek().unwrap_or_default(),
                span: Span::new_with_file_id(self.current_pos, self.current_pos, self.file_id),
            });
        }

        res
    }

    pub fn parse_exact_with_span<T: ParseChars>(&mut self) -> Result<(T, Span), CharLexerError> {
        let checkpoint = self.checkpoint();
        let value = self.parse_exact::<T>()?;
        Ok((value, self.span_since(checkpoint)))
    }

    pub fn attempt<T: ParseChars>(&mut self) -> Result<T, CharLexerError> {
        let checkpoint = self.checkpoint();
        match T::parse(self) {
            Ok(val) => Ok(val),
            Err(e) => {
                self.restore(checkpoint);
                Err(e)
            }
        }
    }

    pub fn optional<T: ParseChars>(&mut self) -> Result<Option<T>, CharLexerError> {
        let checkpoint = self.checkpoint();
        match T::parse(self) {
            Ok(val) => Ok(Some(val)),
            Err(_) => {
                self.restore(checkpoint);
                Ok(None)
            }
        }
    }

    /// Execute a parsing operation with span tracking
    pub fn parse_with<F, T>(&mut self, mut parser: F) -> Result<(T, Span), CharLexerError>
    where
        F: FnMut(&mut Self) -> Result<T, CharLexerError>,
    {
        let start = self.checkpoint();
        let value = parser(self)?;
        let span = Span::new_with_file_id(start.current_pos, self.current_pos, self.file_id);
        Ok((value, span))
    }
}
