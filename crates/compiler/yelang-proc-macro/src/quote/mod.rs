/*!
 * Quasi-quotation for procedural macros.
 */

pub mod to_tokens;

pub use to_tokens::ToTokens;

/// A minimal `quote!` macro for bootstrapping and tests.
///
/// The production `quote!` is a built-in macro in the Yelang compiler. This
/// Rust-level macro supports literal tokens and interpolation via `#ident`.
#[macro_export]
macro_rules! quote {
    // Entry point.
    ($($tt:tt)*) => {{
        let mut __stream = $crate::TokenStream::new();
        $crate::__quote_push!(__stream; $($tt)*);
        __stream
    }};
}

/// Internal helper: push tokens into a stream.
#[macro_export]
macro_rules! __quote_push {
    // Terminal.
    ($stream:ident;) => {};

    // Interpolation: `#ident` where ident is a binding implementing ToTokens.
    ($stream:ident; #$var:ident $($rest:tt)*) => {
        $crate::ToTokens::to_tokens(&$var, &mut $stream);
        $crate::__quote_push!($stream; $($rest)*);
    };

    // Ident token.
    ($stream:ident; $ident:ident $($rest:tt)*) => {
        $stream.push($crate::TokenTree::Ident($crate::Ident::new(
            stringify!($ident),
            $crate::Span::call_site(),
        )));
        $crate::__quote_push!($stream; $($rest)*);
    };

    // Parenthesized group.
    ($stream:ident; ($($inner:tt)*) $($rest:tt)*) => {
        {
            let mut __inner = $crate::TokenStream::new();
            $crate::__quote_push!(__inner; $($inner)*);
            $stream.push($crate::TokenTree::Group($crate::Group::new(
                $crate::Delimiter::Parenthesis,
                __inner,
                $crate::Span::call_site(),
            )));
        }
        $crate::__quote_push!($stream; $($rest)*);
    };

    // Braced group.
    ($stream:ident; {$($inner:tt)*} $($rest:tt)*) => {
        {
            let mut __inner = $crate::TokenStream::new();
            $crate::__quote_push!(__inner; $($inner)*);
            $stream.push($crate::TokenTree::Group($crate::Group::new(
                $crate::Delimiter::Brace,
                __inner,
                $crate::Span::call_site(),
            )));
        }
        $crate::__quote_push!($stream; $($rest)*);
    };

    // Bracketed group.
    ($stream:ident; [$($inner:tt)*] $($rest:tt)*) => {
        {
            let mut __inner = $crate::TokenStream::new();
            $crate::__quote_push!(__inner; $($inner)*);
            $stream.push($crate::TokenTree::Group($crate::Group::new(
                $crate::Delimiter::Bracket,
                __inner,
                $crate::Span::call_site(),
            )));
        }
        $crate::__quote_push!($stream; $($rest)*);
    };

    // Any other single token (literal or punctuation).
    ($stream:ident; $tt:tt $($rest:tt)*) => {
        $crate::__quote_push_tt!($stream; $tt);
        $crate::__quote_push!($stream; $($rest)*);
    };
}

/// Push a literal or punctuation token.
#[macro_export]
macro_rules! __quote_push_tt {
    ($stream:ident; $lit:literal) => {
        $stream.push($crate::TokenTree::Literal($crate::Literal::integer(
            stringify!($lit),
            $crate::Span::call_site(),
        )));
    };
    ($stream:ident; $punct:tt) => {{
        let s = stringify!($punct);
        for ch in s.chars() {
            $stream.push($crate::TokenTree::Punct($crate::Punct::new(
                ch,
                $crate::Spacing::Alone,
                $crate::Span::call_site(),
            )));
        }
    }};
}

// `quote!` is available as `yelang_proc_macro::quote!` through #[macro_export].
