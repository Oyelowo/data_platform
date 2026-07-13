/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

// #[macro_use]
// #[macro_export]
macro_rules! define_tokens{
    ( $( $EnumVariant:ident => $literal:tt ),* $(,)? ) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            pub struct $EnumVariant;

            impl $EnumVariant {
                // pub fn as_token(&self) -> $crate::ast::tokenizer::TokenKind<'_> {
                pub fn as_token(&self) -> super::TokenKind {
                    $crate::tokenizer::TokenKind::$EnumVariant
                }

                pub fn as_str(&self) -> &'static str {
                    stringify!($literal)
                }
            }

            impl ::std::fmt::Display for $EnumVariant {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    write!(f, "{}", stringify!($literal))
                }
            }

            impl ::yelang_lexer::ParseTokenStream<$crate::tokenizer::TokenKind> for $EnumVariant {
                fn parse(stream: &mut ::yelang_lexer::TokenStream<$crate::tokenizer::TokenKind>) -> ::yelang_lexer::TokenResult<Self> {
                    stream.consume($crate::tokenizer::TokenKind::$EnumVariant)?;
                    Ok($EnumVariant)
                }
            }

            impl From<$EnumVariant> for $crate::tokenizer::TokenKind {
                fn from(_: $EnumVariant) -> Self {
                    $crate::tokenizer::TokenKind::$EnumVariant
                }
            }
        )*

        // #[macro_export]
        macro_rules! T{
            $(
                ($literal) => {
                    $crate::tokenizer::tokens_macros::$EnumVariant
                };
            )*
            ('\'') => { $crate::tokenizer::SingleQuote };
            ("'") => { $crate::tokenizer::SingleQuote };
            // Special case for underscore - must come after all other patterns
            ("_") => { $crate::tokenizer::tokens_macros::Underscore };
        }
    }
}

pub(crate) use define_tokens;

define_tokens! {
    // Three-char operators
    DotDotDot => ...,
    DotDotEq => ..=,
    ShiftLeftEqual => <<=,
    ShiftRightEqual => >>=,
    // Two-char operators
    PlusEqual => +=,
    MinusEqual => -=,
    StarEqual => *=,
    SlashEqual => /=,
    PercentEqual => %=,
    CaretEqual => ^=,
    AmpersandEqual => &=,
    PipeEqual => |=,
    LessThanEqual => <=,
    GreaterThanEqual => >=,
    ArrowRight2Lines => "=>",
    EqualEqual => ==,
    BangEqual => !=,
    // LessThanGreaterThan => <>,
    ArrowBoth => "<->",
    ArrowLeft => <-,
    ArrowRight => ->,
    DotDot => ..,
    // Single-char punctuation
    Dot => .,
    Comma => ,,
    Colon => :,
    Semicolon => ;,
    ColonColon => ::,
    OpenParen => '(' ,
    CloseParen => ')',
    OpenBrace => '{',
    CloseBrace => '}',
    OpenBracket => '[',
    CloseBracket => ']',
    // Single-char operators
    Plus => +,
    Minus => -,
    Star => *,
    Slash => /,
    Percent => %,
    Caret => ^,
    Ampersand => &,
    Pipe => |,
    LessThan => <,
    GreaterThan => >,
    Equal => =,
    QuestionMark => ?,
    At => @,
    Hash => #,
    Dollar => $,
    Tilde => ~,
    Backslash => '\\',
    Backtick => '`',
    SingleQuote => '\'',
    DoubleQuote => '"',
    Bang => !,
    And => and,
    Not => not,
    Or => or,
    From_ => from,
    Select => select,
    Create => create,
    Update => update,
    Set => set,
    Upsert => upsert,
    Link => link,
    Unlink => unlink,
    Delete => delete,
    Match => match,
    If => if,
    Else => else,
    In => in,
    While => while,
    Loop => loop,
    Break => break,
    Continue => continue,
    Await => await,
    Return => return,
    For => for,
    Impl => impl,
    Dyn => dyn,
    Pub => pub,
    Struct => struct,
    Enum => enum,
    Trait => trait,
    Async => async,
    Mut => mut,
    Mod => mod,
    Gen => gen,
    Let => let,
    Static => static,
    Crate => crate,
    Const => const,
    Use => use,
    Super => super,
    SelfKw => self,
    Fn => fn,
    TypeToken => type,
    DefaultKw => default,
    TypeOf => typeof,
    Where => where,
    As => as,
    Is => is,
    BeginTransaction => begin,
    CancelTransaction => transaction,
    CommitTransaction => commit,
    ReturnType => ReturnType,
    Parameters => Parameters,
    Pick => Pick,
    Omit => Omit,
    Asc => asc,
    Desc => desc,
    Order => order,
    By => by,
    Into => into,
    Start => start,
    Limit => limit,
    RangeKw => range,
    HopsKw => hops,
    Enumerate => enumerate,
    Distinct => distinct,
    Links => links,
    Group => group,
}

// Manual definition for Underscore since _ is a catch-all pattern in macros
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Underscore;

impl Underscore {
    pub fn as_token(&self) -> super::TokenKind {
        super::TokenKind::Underscore
    }

    pub fn as_str(&self) -> &'static str {
        "_"
    }
}

impl ::std::fmt::Display for Underscore {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        write!(f, "_")
    }
}

impl yelang_lexer::ParseTokenStream<super::TokenKind> for Underscore {
    fn parse(
        stream: &mut yelang_lexer::TokenStream<super::TokenKind>,
    ) -> yelang_lexer::TokenResult<Self> {
        stream.consume(super::TokenKind::Underscore)?;
        Ok(Underscore)
    }
}

impl From<Underscore> for super::TokenKind {
    fn from(_: Underscore) -> Self {
        super::TokenKind::Underscore
    }
}

pub(crate) use T;

// pub use Token;
// pub use crate::ast::tokenizer::tokens_macros::Token;

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use rstest::rstest;
//
//     #[rstest]
//     #[case(".", Symbol::Dot)]
//     #[case("..", Symbol::Dot)]
//     #[case("...", Symbol::Dot)]
//     #[case(". .", Symbol::Dot)]
//     #[case("./.", Symbol::Dot)]
//     fn test_dot(#[case] input: &str, #[case] expected: Symbol) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse::<T!["."]>().unwrap();
//         assert_eq!(res.as_symbol(), expected);
//     }
//
//     #[rstest]
//     #[case("..")]
//     #[case("...")]
//     #[case(". .")]
//     #[case("./.")]
//     fn test_dot_exact(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse_exact::<T!["."]>();
//         assert!(res.is_err());
//     }
//
//     #[rstest]
//     #[case(";")]
//     #[case(" ")]
//     #[case("x")]
//     fn test_dot_fail(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse::<Dot>();
//         assert!(res.is_err());
//         let res = cursor.parse::<T!["."]>();
//         assert!(res.is_err());
//     }
//
//     #[rstest]
//     #[case("->", Symbol::ArrowRight)]
//     #[case("->x", Symbol::ArrowRight)]
//     #[case("->->", Symbol::ArrowRight)]
//     fn test_arrow_right(#[case] input: &str, #[case] expected: Symbol) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse::<ArrowRight>().unwrap();
//         assert_eq!(res.as_symbol(), expected);
//
//         cursor.reset_dangerous();
//         let res = cursor.parse::<T!["->"]>().unwrap();
//         assert_eq!(res.as_symbol(), expected);
//     }
//
//     #[rstest]
//     #[case("->", vec![ArrowRight])]
//     #[case("->->->", vec![ArrowRight, ArrowRight, ArrowRight])]
//     #[case("->->", vec![ArrowRight, ArrowRight])]
//     fn test_arrow_right_repeated(#[case] input: &str, #[case] expected: Vec<ArrowRight>) {
//         use crate::ast::lexer::Repeat;
//
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse::<Repeat<ArrowRight>>().unwrap();
//         assert_eq!(res.value_owned(), expected);
//
//         cursor.reset_dangerous();
//         let res = cursor.parse::<Repeat<T!["->"]>>().unwrap();
//         assert_eq!(res.value_owned(), expected);
//     }
//
//     #[rstest]
//     #[case("-->")]
//     #[case("->x")]
//     #[case("->->")]
//     fn test_arrow_right_exact(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse_exact::<ArrowRight>();
//         assert!(res.is_err());
//
//         cursor.reset_dangerous();
//         let res = cursor.parse_exact::<T!["->"]>();
//         assert!(res.is_err());
//     }
//
//     #[rstest]
//     #[case("<-")]
//     #[case("<->")]
//     #[case("-->")]
//     #[case("-->>")]
//     #[case("-->>")]
//     #[case("--")]
//     #[case(">")]
//     #[case(";")]
//     #[case("..")]
//     #[case("...")]
//     #[case(". .")]
//     #[case("./.")]
//     fn test_arrow_right_fail(#[case] input: &str) {
//         let mut cursor = Cursor::new(input);
//         let res = cursor.parse::<ArrowRight>();
//         assert!(res.is_err());
//
//         cursor.reset_dangerous();
//         let res = cursor.parse::<T!["->"]>();
//         assert!(res.is_err());
//     }
// }
//
