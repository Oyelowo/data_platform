/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 09/02/2025
 */
use std::sync::Arc;

use crate::{
    Empty, FloatSuffix, IntSuffix, ParseTokenStream, Symbol, TokenResult, TokenStream,
    consume_token,
};

use super::TokenKind;

/// Literal values in expressions
///
/// Represents all forms of literal constants including primitives,
/// strings, and domain-specific tagged literals like regex and datetime.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// Integer literal: `42`, `0xFF`, `1_000_000`
    ///
    /// # Example
    /// ```
    /// let x = 42;
    /// let y: i64 = 100;
    /// ```
    Int(IntegerLit),

    /// Floating point literal: `3.14`, `2.0f32`
    ///
    /// # Example
    /// ```
    /// let pi = 3.14159;
    /// let x: f32 = 1.5;
    /// ```
    Float(FloatLit),

    /// Boolean literal: `true` or `false`
    ///
    /// # Example
    /// ```
    /// let flag = true;
    /// let enabled: bool = false;
    /// ```
    Bool(bool),

    /// Character literal: `'a'`, `'ñ'`, `'🦀'`
    ///
    /// # Example
    /// ```
    /// let ch = 'x';
    /// let emoji = '🎉';
    /// ```
    Char(char),

    // ===== String Types =====
    /// Simple string literal: `"hello"`
    ///
    /// # Example
    /// ```
    /// let msg = "Hello, World!";
    /// ```
    Str(StringLit),

    // /// Template string with interpolation: `` `Hello ${name}!` ``
    // ///
    // /// # Example
    // /// ```
    // /// let name = "Alice";
    // /// let greeting = `Hello ${name}!`; // "Hello Alice!"
    // /// ```
    // Template(Vec<TemplatePart>),

    // ===== Tagged Literals for Complex Built-in Types =====
    /// Regular expression literal: `/pattern/flags`
    ///
    /// # Example
    /// ```
    /// let email_regex = /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/;
    /// let case_insensitive = /hello/i;
    /// ```
    Regex(RegexLit),

    /// DateTime literal: `dt'2025-11-12'`, `dt'2025-11-12T10:30:00Z'`
    ///
    /// # Example
    /// ```
    /// let date = dt'2025-11-12';
    /// let timestamp = dt'2025-11-12T10:30:00Z';
    /// ```
    DateTime(DateTimeLit),

    /// Duration literal: `5h30m`, `2d`, `1h15m30s`
    ///
    /// # Example
    /// ```
    /// let timeout = 5m;
    /// let cache_ttl = 1h30m;
    /// ```
    Duration(DurationLit),

    /// Byte array literal: `b"bytes"`
    ///
    /// # Example
    /// ```
    /// let data = b"hello";
    /// ```
    Bytes(Arc<[u8]>),

    /// UUID literal: `550e8400-e29b-41d4-a716-446655440000`
    ///
    /// # Example
    /// ```
    /// let id = 550e8400-e29b-41d4-a716-446655440000;
    /// ```
    Uuid(UuidLit),

    /// Geospatial literal: `POINT(10 20)`, `LINESTRING(10 20, 30 40)`
    ///
    /// # Example
    /// ```
    /// let location = POINT(10 20);
    /// let path = LINESTRING(10 20, 30 40);
    /// ```
    Geometry(GeometryLit),

    /// Record ID literal: `user:123`, `product:name`
    ///
    /// # Example
    /// ```
    /// let user_id = user:123;
    /// let product_id = product:name;
    /// ```
    RecordId(RecordIdLit),

    // ===== Special Values =====
    /// Unit value: `()`
    ///
    /// Represents the absence of a value, similar to `void` in other languages
    ///
    /// # Example
    /// ```
    /// fn do_nothing() -> () { }
    /// ```
    Unit,
}

/// Regular expression literal with pattern and flags
///
/// # Example
/// ```
/// /^[a-z]+$/i  // pattern: "^[a-z]+$", flags: "i"
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegexLit {
    /// The regex pattern
    pub pattern: Empty,
    /// Optional flags (e.g., "i" for case-insensitive, "g" for global)
    pub flags: Option<Symbol>,
}

/// DateTime literal with value and optional format
///
/// # Example
/// ```
/// dt'2025-11-12'
/// dt'2025-11-12T10:30:00Z'
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DateTimeLit {
    /// The datetime value as a string
    pub value: Symbol,
    /// Optional format hint for parsing
    pub format: Option<Symbol>,
}

/// Duration literal
///
/// # Example
/// ```
/// 5h30m      // 5 hours, 30 minutes
/// 2d         // 2 days
/// 1h15m30s   // 1 hour, 15 minutes, 30 seconds
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DurationLit {
    /// The duration value as a string
    pub value: Symbol,
}

/// Byte array literal
///
/// # Example
/// ```
/// b"hello"
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ByteLit {
    /// The byte array value
    pub value: Vec<u8>,
}

/// UUID literal
///
/// # Example
/// ```
/// 550e8400-e29b-41d4-a716-446655440000
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UuidLit {
    /// The UUID value as a string
    pub value: Symbol,
}

/// Geometry literal
///
/// # Example
/// ```
/// POINT(10 20)
/// LINESTRING(10 20, 30 40)
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometryLit {
    /// The geometry value as a string
    pub value: Symbol,
}

/// Record ID literal
///
/// # Example
/// ```
/// user:123
/// product:name
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordIdLit {
    /// The record ID value as a string
    pub value: Symbol,
}

/// Integer literal with optional type suffix
///
/// # Example
/// ```
/// 42i32    // 32-bit signed integer
/// 100u64   // 64-bit unsigned integer
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntegerLit {
    /// The integer value
    pub value: Symbol,
    /// Optional type suffix
    pub suffix: Option<IntSuffix>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for IntegerLit {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let lit = consume_token!(stream, TokenKind::Lit(Literal::Int(lit)) => lit);
        Ok(*lit)
    }
}

/// Floating point literal with optional type suffix
///
/// # Example
/// ```
/// 3.14f32   // 32-bit float
/// 2.0f64    // 64-bit float
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatLit {
    /// The float value
    pub value: Symbol,
    /// Optional type suffix
    pub suffix: Option<FloatSuffix>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrKind {
    /// Regular string literal with escape sequences
    Normal,
    /// Raw string literal without escape processing
    Raw {
        /// Number of `#` used for raw string delimiters
        hash_count: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StringLit {
    /// The string value
    pub value: Symbol,
    pub kind: StrKind,
}

/// Part of a template string
///
/// Template strings can contain both static text and interpolated expressions
///
/// # Example
/// ```
/// `Hello ${name}!` -> [String("Hello "), Expr(name), String("!")]
/// ```
impl Literal {}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Literal {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let lit = consume_token!(stream, TokenKind::Lit(lit) => lit);
        Ok(lit.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::{TokenKind, TokenizeChars};

    #[test]
    fn parse_literal() {
        // let input = "##'-45'#.34e-56'##";
        let input = "4..99 345.45e-6 'this is my string'";
        // let input = "[1, 2, [5, 4, [9, 2] ], 6]";
        // let input = "[1, 2]";
        // let input = "[1, 2, [5, 4], 6]";
        // let input = "[1, {score: 100, nested: {another:  999}, }, null, true, false, 5..69, 2,
        //  [5, 'this is some string,  with comma', 4, [9,7], ], 3, 8, 54.45E-13]";
        // let input = "[1,2, {}, [5, 4], 6];";
        // let input = "[1,2, {name : 5, info :{ score: 6, country: 'canada'}}, [5, 4], 6];";
        // let input = "[1,2, [5, 4], 6];";
        // let input = "[null,2, 6,]";
        // let input = "[6 , 4,]";
        // let input = "[6, 2]";
        // let input = "[[null, 45]]";
        // let input = "[3..6]";
        // let input = "[{ma:6, nested: {another:  999}, arr: [5, 'this is some string,  with comma', 4, [9,7], ]}]";
        // let input = "{ma:6, nested: {another:  999}, arr: [5, 'this is some string,  with comma', 4, [9,7], ]}";
        // let mut tokens = Token::tokenize(input)
        //     .inspect(|t| {
        //         // print!("print{}", t);
        //     })
        //     .unwrap();
        //
        // let xx = tokens
        //     .parse::<Literal>()
        //     .inspect(|t| {
        //         // panic!("xxx{}", t);
        //     })
        //     .inspect_err(|e| {
        //         // panic!("errorrr{}", e);
        //     })
        //     .unwrap();
    }
    use super::*;

    // #[test]
    // fn test_parse_object() {
    //     let input = r#"{ "name": "John Doe", "age": 30, "active": true, "address": { "street": "123 Main St" } }"#;
    //     let mut cursor = CharCursor::new(input);
    //     let tokens = crate::lexer::token_iter_to_token_stream::<Token>(
    //         crate::lexer::CharLexerIter::new(&mut cursor),
    //         Span::default(),
    //     );
    //
    //
    //     // let tokens = ""
    //     //     CharLexerIter::new(&mut cursor)
    //     //     .filter_map(Result::ok)
    //     //     .collect::<Vec<_>>();
    //     // let mut stream = TokenStream {
    //     //     tokens,
    //     //     position: 0,
    //     //     current_span: Span::default(),
    //     // };
    //
    //     let object = tokens.parse::<Object>();
    //
    //     // println!("{:#?}", object);
    //     assert!(object.is_ok());
    //
    //     let object = object.unwrap();
    //     assert_eq!(object.fields.len(), 4);
    //     assert_eq!(object.fields[0].0 .0, "name");
    //     assert_eq!(
    //         object.fields[0].1,
    //         Value::String(StringLit::new_unchecked("John Doe"))
    //     );
    //     assert_eq!(object.fields[1].0 .0, "age");
    //     assert_eq!(object.fields[1].1, Value::Int(30));
    //     assert_eq!(object.fields[2].1, Value::Bool(true));
    //     assert!(matches!(object.fields[3].1, Value::Object(_)));
    // }
    //
    // #[test]
    // fn test_parse_array() {
    //     let input = r#"[1, 2, 3, "hello", { "x": 1 }, [true, false]]"#;
    //     let mut cursor = CharCursor::new(input);
    //     let tokens = crate::lexer::token_iter_to_token_stream::<Token>(
    //         crate::lexer::CharLexerIter::new(&mut cursor),
    //         Span::default(),
    //     );
    //     let array = tokens.parse::<Array>();
    //     // println!("{:#?}", array);
    //     assert!(array.is_ok());
    //     let array = array.unwrap();
    //     assert_eq!(array.elements.len(), 6);
    //     assert_eq!(array.elements[0], Value::Int(1));
    //     assert_eq!(array.elements[3], Value::String(StringLit::new_unchecked("hello")));
    //     assert!(matches!(array.elements[4], Value::Object(_)));
    //     assert!(matches!(array.elements[5], Value::Array(_)));
    //
    // }
    //
    // #[test]
    // fn test_parse_empty_object() {
    //     let input = r#"{}"#;
    //     let mut cursor = CharCursor::new(input);
    //     let tokens = crate::lexer::token_iter_to_token_stream::<Token>(
    //         crate::lexer::CharLexerIter::new(&mut cursor),
    //         Span::default(),
    //     );
    //     let object = tokens.parse::<Object>();
    //     assert!(object.is_ok());
    //     assert_eq!(object.unwrap().fields.len(), 0);
    // }
    //
    // #[test]
    // fn test_parse_empty_array() {
    //     let input = r#"[]"#;
    //      let mut cursor = CharCursor::new(input);
    //     let tokens = crate::lexer::token_iter_to_token_stream::<Token>(
    //         crate::lexer::CharLexerIter::new(&mut cursor),
    //         Span::default(),
    //     );
    //     let array = tokens.parse::<Array>();
    //     assert!(array.is_ok());
    //     assert_eq!(array.unwrap().elements.len(), 0);
    // }
    // #[test]
    // fn test_parse_nested_object() {
    // let input = r#"{ "a": { "b": { "c":
}
