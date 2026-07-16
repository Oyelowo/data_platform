/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */

use crate::BytePosition;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Add};

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash,
)]
#[repr(transparent)]
pub struct FileId(u32);

impl FileId {
    pub const UNKNOWN: FileId = FileId(0);

    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct Position {
    /// 1-based line number
    pub line: u32,
    /// 1-based column number
    pub column: u32,
    /// 0-based byte offset in input
    pub absolute: usize,
}

impl Position {
    pub fn min(self, other: Position) -> Position {
        if self < other { self } else { other }
    }

    pub fn max(self, other: Position) -> Position {
        if self > other { self } else { other }
    }

    /// Create a Position from a BytePosition (approximate)
    pub fn from_byte(byte_pos: BytePosition) -> Self {
        // Note: This is a simplified conversion
        Position {
            // TODO: Reconsider how line and column are calculated
            line: 1,
            column: 1,
            absolute: byte_pos.absolute,
        }
    }
}

impl Add<usize> for Position {
    type Output = Position;

    fn add(self, rhs: usize) -> Self::Output {
        Self {
            line: self.line,
            column: self.column + rhs as u32,
            absolute: self.absolute + rhs,
        }
    }
}

impl Default for Position {
    fn default() -> Self {
        Self {
            line: 1,
            column: 1,
            absolute: 0,
        }
    }
}

impl Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// The raw value used for the root hygiene context. Matches
/// `yelang_macro_core::SyntaxContextId::default()`.
pub const ROOT_SYNTAX_CONTEXT: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Span {
    pub(super) start: Position,
    pub(super) end: Position,
    pub(super) file_id: FileId,
    /// Hygiene context ID. Stored as a raw `u32` to keep `yelang-lexer`
    /// independent of `yelang-macro-core`. The root context is 1.
    pub(super) syntax_context: u32,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            start: Position::default(),
            end: Position::default(),
            file_id: FileId::default(),
            syntax_context: ROOT_SYNTAX_CONTEXT,
        }
    }
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Span")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("syntax_context", &self.syntax_context)
            .finish()
    }
}

impl Span {
    pub fn new(start: Position, end: Position) -> Self {
        Self {
            start,
            end,
            file_id: FileId::default(),
            syntax_context: ROOT_SYNTAX_CONTEXT,
        }
    }

    pub fn new_with_file_id(start: Position, end: Position, file_id: FileId) -> Self {
        Self {
            start,
            end,
            file_id,
            syntax_context: ROOT_SYNTAX_CONTEXT,
        }
    }

    pub fn with_syntax_context(self, syntax_context: u32) -> Self {
        Self {
            syntax_context,
            ..self
        }
    }

    pub fn default_with_file_id(file_id: FileId) -> Self {
        Self {
            start: Position::default(),
            end: Position::default(),
            file_id,
            syntax_context: ROOT_SYNTAX_CONTEXT,
        }
    }

    pub fn file_id(&self) -> FileId {
        self.file_id
    }

    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(
            self.file_id, other.file_id,
            "attempted to merge spans from different files"
        );
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            file_id: self.file_id,
            syntax_context: self.syntax_context,
        }
    }

    pub fn len(&self) -> usize {
        self.end.absolute - self.start.absolute
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn is_valid(&self) -> bool {
        self.start.absolute <= self.end.absolute
    }

    pub(crate) fn is_default(&self) -> bool {
        self.start == Position::default()
            && self.end == Position::default()
            && self.syntax_context == ROOT_SYNTAX_CONTEXT
    }

    pub fn start(&self) -> Position {
        self.start
    }

    pub fn end(&self) -> Position {
        self.end
    }

    pub fn syntax_context(&self) -> u32 {
        self.syntax_context
    }
}

impl Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}..{}", self.start.absolute, self.end.absolute)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Checkpoint {
    pub(super) current_pos: Position,
}

impl Checkpoint {
    pub fn current_pos(&self) -> Position {
        self.current_pos
    }
}
