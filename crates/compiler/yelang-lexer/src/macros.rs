//! Parser/lexer helper macros.
//!
//! Kept out of `lexer/mod.rs` so that `mod.rs` can remain a pure facade.

/// The `try_parse!` macro takes one or more parser expressions
/// and returns the result of the first one that succeeds.
/// It hides the `.or_else(|_| ...)` chaining so that you only list the alternatives.
#[macro_export]
macro_rules! try_parse {
    ($first:expr $(, $rest:expr)+ $(,)?) => {{
         $first
        $(
             .or_else(|_| $rest)
        )+
    }};
    ($first:expr $(,)?) => {
        $first
    };
}

/// The `match_map!` macro takes a token stream and a list of parser-mapper pairs.
/// It tries each parser in order, and if one succeeds, it applies the corresponding mapper.
#[macro_export]
macro_rules! match_map {
    // Internal helper with checkpoint parameter
    (@inner $stream:ident, $checkpoint:ident, $parser:ty => $map:expr) => {
        $stream.parse::<$parser>().map($map)
    };
    (@inner $stream:ident, $checkpoint:ident, $first:ty => $first_map:expr, $($rest:ty => $rest_map:expr),+ $(,)?) => {
        $stream.parse::<$first>()
            .map($first_map)
            .or_else(|_| {
                $stream.restore($checkpoint);
                match_map!(@inner $stream, $checkpoint, $($rest => $rest_map),+)
            })
    };
    // Public entry point: single parser => mapper
    ($stream:ident, $parser:ty => $map:expr) => {
        $stream.parse::<$parser>().map($map)
    };
    // Public entry point: multiple parsers - creates checkpoint once
    ($stream:ident, $first:ty => $first_map:expr, $($rest:ty => $rest_map:expr),+ $(,)?) => {{
        let __checkpoint = $stream.checkpoint();
        match_map!(@inner $stream, __checkpoint, $first => $first_map, $($rest => $rest_map),+)
    }};
}

/// Like `match_map!`, but the mapper is allowed to fail (i.e. return `TokenResult<_>`).
#[macro_export]
macro_rules! match_map_res {
    // Internal helper with checkpoint parameter
    (@inner $stream:ident, $checkpoint:ident, $parser:ty => $map:expr) => {
        $stream.parse::<$parser>().and_then($map)
    };
    (@inner $stream:ident, $checkpoint:ident, $first:ty => $first_map:expr, $($rest:ty => $rest_map:expr),+ $(,)?) => {
        $stream.parse::<$first>()
            .and_then($first_map)
            .or_else(|_| {
                $stream.restore($checkpoint);
                match_map_res!(@inner $stream, $checkpoint, $($rest => $rest_map),+)
            })
    };
    // Public entry point: single parser => mapper
    ($stream:ident, $parser:ty => $map:expr) => {
        $stream.parse::<$parser>().and_then($map)
    };
    // Public entry point: multiple parsers - creates checkpoint once
    ($stream:ident, $first:ty => $first_map:expr, $($rest:ty => $rest_map:expr),+ $(,)?) => {{
        let __checkpoint = $stream.checkpoint();
        match_map_res!(@inner $stream, __checkpoint, $first => $first_map, $($rest => $rest_map),+)
    }};
}

#[macro_export]
macro_rules! token_mapper_inner {
    ($cursor:expr, $checkpoint:expr, $first:expr, $($rest:expr),+ $(,)?) => {{
         $first.or_else(|_| {
              // Restore to the same checkpoint before the next attempt.
              $cursor.restore($checkpoint);
            $crate::token_mapper_inner!($cursor, $checkpoint, $($rest),+)
         })
    }};
    ($cursor:expr, $checkpoint:expr, $attempt:expr $(,)?) => {
         $attempt
    }
}

#[macro_export]
macro_rules! token_mapper {
    ($cursor:expr, $($attempt:expr),+ $(,)?) => {{
         let __checkpoint = $cursor.checkpoint();
        $crate::token_mapper_inner!($cursor, __checkpoint, $($attempt),+)
    }};
}

/// Consume a token from the stream and destructure it with a pattern.
#[macro_export]
macro_rules! consume_token {
    ($stream:expr, $pattern:pat => $value:expr) => {{
        #[allow(unused_variables)]
        let token = $stream
            .consume_token_fn(|t| matches!(t.kind(), $pattern))?
            .kind();

        match token {
            #[allow(unused_variables)]
            $pattern => $value,
            _ => unreachable!("consume_token_fn guarantees the correct variant"),
        }
    }};
}

#[macro_export]
macro_rules! consume_variant {
    ($stream:expr, $variant:ident ( $($field:ident),+ $(,)? )) => {
        let Token::$variant ( $($field),+ ) = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
    ($stream:expr, $enum:ident::$variant:ident ( $($field:ident),+ $(,)? )) => {
        let $enum::$variant ( $($field),+ ) = $stream
            .consume_token_fn(|t| matches!(t.kind(), $enum::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };

    ($stream:expr, $variant:ident { $($field:ident),+ $(,)? }) => {
        let Token::$variant { $($field),+ , ..} = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
    ($stream:expr, $enum:ident:: $variant:ident { $($field:ident),+ $(,)? }) => {
        let $enum::$variant { $($field),+ , ..} = $stream
            .consume_token_fn(|t| matches!(t.kind(), $enum::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };

    ($stream:expr, $variant:ident { $($field:ident : $reassign:ident),+ $(,)? }) => {
        let Token::$variant { $($field: $reassign),+ , ..} = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
    ($stream:expr, $enum:ident:: $variant:ident { $($field:ident : $reassign:ident),+ $(,)? }) => {
        let $enum::$variant { $($field: $reassign),+ , ..} = $stream
            .consume_token_fn(|t| matches!(t.kind(), $enum::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };

    ($stream:expr, $variant:ident) => {
        let Token::$variant = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
    ($stream:expr, $enum:ident::$variant:ident) => {
        let $enum::$variant = $stream
            .consume_token_fn(|t| matches!(t.kind(), $enum::$variant))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
}

#[macro_export]
macro_rules! consume_variant_return {
    ($stream:expr, $variant:ident ( $($field:ident),+ $(,)? )) => {{
        let Token::$variant ( $($field),+ ) = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
        $($field),+
    }};
    ($stream:expr, $variant:ident { $($field:ident),+ $(,)? }) => {{
        let Token::$variant { $($field),+ , ..} = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant { .. }))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
        ($($field),+)
    }};

    ($stream:expr, $variant:ident) => {
        let Token::$variant = $stream
            .consume_token_fn(|t| matches!(t.kind(), Token::$variant))?
            .kind()
            else { unreachable!("Expected token variant Token::{}", stringify!($variant)) };
    };
}
