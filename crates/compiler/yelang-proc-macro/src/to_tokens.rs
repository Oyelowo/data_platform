//! Conversion to tokens for `quote!` interpolation.

use crate::{Ident, Literal, TokenStream, TokenTree};

/// Types that can be interpolated into `quote!`.
pub trait ToTokens {
    fn to_tokens(&self, stream: &mut TokenStream);

    fn to_token_stream(&self) -> TokenStream {
        let mut s = TokenStream::new();
        self.to_tokens(&mut s);
        s
    }
}

impl ToTokens for TokenTree {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.push(self.clone());
    }
}

impl ToTokens for TokenStream {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.extend(self.clone());
    }
}

impl ToTokens for Ident {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.push(TokenTree::Ident(self.clone()));
    }
}

impl ToTokens for Literal {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.push(TokenTree::Literal(self.clone()));
    }
}

impl ToTokens for crate::Punct {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.push(TokenTree::Punct(*self));
    }
}

impl ToTokens for crate::Group {
    fn to_tokens(&self, stream: &mut TokenStream) {
        stream.push(TokenTree::Group(self.clone()));
    }
}

impl ToTokens for str {
    fn to_tokens(&self, stream: &mut TokenStream) {
        Literal::string(self, crate::Span::call_site()).to_tokens(stream);
    }
}

impl ToTokens for String {
    fn to_tokens(&self, stream: &mut TokenStream) {
        self.as_str().to_tokens(stream);
    }
}

impl<T: ToTokens + ?Sized> ToTokens for &T {
    fn to_tokens(&self, stream: &mut TokenStream) {
        (**self).to_tokens(stream);
    }
}

impl<T: ToTokens + ?Sized> ToTokens for &mut T {
    fn to_tokens(&self, stream: &mut TokenStream) {
        (**self).to_tokens(stream);
    }
}

impl<T: ToTokens> ToTokens for Option<T> {
    fn to_tokens(&self, stream: &mut TokenStream) {
        if let Some(value) = self {
            value.to_tokens(stream);
        }
    }
}

impl<T: ToTokens> ToTokens for Vec<T> {
    fn to_tokens(&self, stream: &mut TokenStream) {
        for item in self {
            item.to_tokens(stream);
        }
    }
}

impl<T: ToTokens> ToTokens for [T] {
    fn to_tokens(&self, stream: &mut TokenStream) {
        for item in self {
            item.to_tokens(stream);
        }
    }
}

macro_rules! impl_to_tokens_int {
    ($($t:ty),*) => {
        $(
            impl ToTokens for $t {
                fn to_tokens(&self, stream: &mut TokenStream) {
                    Literal::integer(self.to_string(), crate::Span::call_site()).to_tokens(stream);
                }
            }
        )*
    };
}

impl_to_tokens_int!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);
