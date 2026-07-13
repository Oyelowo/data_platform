/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/03/2025
 */

use super::{CallArgument, Expr};
use crate::{AngleBracketedArgs, GenericArgs, Ident, PathSegment, T};
use yelang_lexer::{ArrayCreator, ParseTokenStream, TokenResult, TokenStream};

#[derive(Debug, Clone, PartialEq)]
pub struct MethodCallExpr {
    pub receiver: Box<Expr>, // e.g., "hello" or u.name
    /// The method name and its generic arguments, e.g. `foo::<Bar, Baz>`.
    pub segment: PathSegment,
    // pub method: Ident,                // e.g., "lower", "trim", "length"
    pub arguments: Vec<CallArgument>, // Optional args for method
                                      // pub is_null_safe: bool,               // Indicates if the method is null-safe
                                      // pub span: Span,
}

// "user.name".lower()
// u.email.trim()
// b.title.replace(" ", "_")

// Expr::MethodCall(MethodCallExpr {
//     receiver: Expr::Literal("HELLO"),
//     method: "lower",
//     arguments: []
// })

// users@u[*].{
//   name,
//   blogs: u.blogs@b[where maths.round(b.views / 10) > 80].{
//     title,
//     tech_tags: b.tags@t[where t.kind == "tech"][*].name.map((n) => n.upper())
//   },
//   lower_name: u.name.lower()
// }

impl ParseTokenStream<crate::tokenizer::TokenKind> for MethodCallExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Expect: <expr> "." <ident> "(" arg-list ")"
        let checkpoint = stream.checkpoint();

        // First parse the "receiver" expression.
        let receiver_expr = stream.parse::<Expr>()?;

        // Then parse the dot + method name
        stream.parse::<T![.]>()?;

        // In expression position, require Rust-like turbofish `::<...>` to attach
        // angle-bracketed generic args, to avoid `<` ambiguity.
        let method_ident = stream.parse::<Ident>()?;
        let method_args = {
            let cp = stream.checkpoint();
            if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                Some(GenericArgs::AngleBracketed(ab))
            } else {
                stream.restore(cp);
                None
            }
        };
        let method = PathSegment {
            ident: method_ident,
            args: method_args,
        };

        // Then parse parentheses for arguments
        let (args_list, _args_span) =
            stream.parse_with_span::<ArrayCreator<T!['('], CallArgument, T![,], T![')']>>()?;
        let arguments = args_list.items_owned();

        Ok(MethodCallExpr {
            receiver: Box::new(receiver_expr),
            segment: method,
            arguments,
        })
    }
}

impl MethodCallExpr {}
