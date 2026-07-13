/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::Symbol;
use std::sync::Arc;

use crate::{Comment, Token};

use super::{Ident, Literal, StrKind};

#[derive(Debug, Clone, PartialEq)]
pub enum InterpolatedPart {
    Literal(Symbol),
    Expression(Arc<[Token<crate::tokenizer::TokenKind>]>),
}

#[derive(Clone, PartialEq)]
pub enum TokenKind {
    Select,
    From_,
    Where,
    Struct,
    Enum,
    Trait,
    Group,
    By,
    Order,
    Into,
    Let,
    Fn,
    TypeToken,
    DefaultKw,
    TypeOf,
    ReturnType,
    Parameters,
    Pick,
    Omit,
    Pub,
    As,
    Or,
    Mod,
    Mut,
    CreateIndex,
    Create,
    Crate,
    SelfKw,
    SelfType,
    Super,
    Pkg,
    Const,
    Static,
    Update,
    Set,
    Insert,
    Impl,
    Dyn,
    Delete,
    For,
    Link,
    Unlink,
    Upsert,
    BeginTransaction,
    CommitTransaction,
    CancelTransaction,
    Enumerate,
    Distinct,
    Match,
    If,
    Else,
    While,
    Loop,
    Async,
    Gen,
    Await,
    Continue,
    Break,
    Yield,
    Return,
    Links,
    And,
    Not,
    Xor,
    Is,
    In,
    On,
    Asc,
    Start,
    Limit,
    RangeKw,
    HopsKw,
    Desc,
    Use,
    /// Reserved keyword: `null` is not supported at the language surface.
    ///
    /// It is tokenized so we can provide a clear diagnostic instead of treating it as an identifier.
    Null,
    //
    Ident(Ident),
    Lit(Literal),
    InterpolatedString {
        parts: Vec<InterpolatedPart>,
        kind: StrKind,
    },
    /// Trivia: whitespace (spaces, tabs, newlines).
    ///
    /// Only produced by the lossless tokenizer used for tolerant syntax.
    Whitespace(Symbol),
    Comment(Comment),

    // Compound punctuation
    /// ...
    DotDotDot,
    /// ..=
    DotDotEq,
    /// <<=
    ShiftLeftEqual,
    /// >>=
    ShiftRightEqual,
    /// +=
    PlusEqual,
    /// -=
    MinusEqual,
    /// *=
    StarEqual,
    /// /=
    SlashEqual,
    /// %=
    PercentEqual,
    /// ^=
    CaretEqual,
    /// &=
    AmpersandEqual,
    /// |=
    PipeEqual,
    /// <=
    LessThanEqual,
    /// >=
    GreaterThanEqual,
    /// ==
    EqualEqual,
    /// !=
    BangEqual,
    /// <->
    ArrowBoth,
    /// <-
    ArrowLeft,
    /// ->
    ArrowRight,
    /// =>
    ArrowRight2Lines,
    /// ..
    DotDot,
    // Single-char punctuation
    /// .
    Dot,
    /// ,
    Comma,
    /// :
    Colon,
    /// ::
    ColonColon,
    /// ;
    Semicolon,
    /// (
    OpenParen,
    /// )
    CloseParen,
    /// {
    OpenBrace,
    /// }
    CloseBrace,
    /// [
    OpenBracket,
    /// ]
    CloseBracket,
    // Single-char operators
    /// +
    Plus,
    /// -
    Minus,
    /// *
    Star,
    /// /
    Slash,
    /// %
    Percent,
    /// ^
    Caret,
    /// &
    Ampersand,
    /// |
    Pipe,
    /// !
    Bang,
    /// <
    LessThan,
    /// >
    GreaterThan,
    /// =
    Equal,
    /// ?
    QuestionMark,
    /// @
    At,
    /// #
    Hash,
    /// $
    Dollar,
    /// ~
    Tilde,
    /// \
    Backslash,
    /// `
    Backtick,
    /// '
    SingleQuote,
    /// "
    DoubleQuote,
    /// 'identifier - lifetime or label like 'outer, 'static, 'a
    Lifetime(Symbol),
    /// _
    Underscore,
    /// -
    Hyphen,

    Unknown,
}
