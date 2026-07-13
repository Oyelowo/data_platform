use super::*;
use crate::Ident;
use crate::T;
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// A field in a structural type
///
/// # Example
/// ```
/// { name: string, age: i32 }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StructuralField {
    /// Field name
    pub name: Ident,
    /// Field type
    pub ty: Type,
    /// Whether the field is optional
    pub optional: bool,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for StructuralField {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (name, _colon, ty) = stream.parse::<(Ident, T![:], Type)>()?;
        Ok(StructuralField {
            name,
            ty,
            optional: false,
        })
    }
}
