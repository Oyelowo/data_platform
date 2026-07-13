/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 26/01/2025
 */
use super::{TokenStream, TokenTrait, error::TokenResult};

pub trait ParseTokenStream<TKind, TOutput = Self>
where
    TKind: TokenTrait,
    Self: Sized,
{
    // type Output;
    fn parse(stream: &mut TokenStream<TKind>) -> TokenResult<TOutput>;
}

// pub trait ParseTokenStream<'a>: Sized {
//     fn parse(cursor: &mut TokenStream<'a>) -> TokenResult<Self>;
//     // fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(Output, Span), TokenError>;
//
//     // Zero or more matches
//     // fn many0(self) -> Many0<Self> {
//     //     Many0 { parser: self }
//     // }
//     //
//     // /// One or more matches
//     // fn many1(self) -> Many1<Self> {
//     //     Many1 { parser: self }
//     // }
//     //
//     // /// Optional match
//     // fn optional(self) -> Optional<Self> {
//     //     Optional { parser: self }
//     // }
//     //
//     // /// Separated list
//     // fn separated_list<T>(self, separator: T) -> SeparatedList<Self, T>
//     // where
//     //     T: ParseTokenStream<'a, ()>,
//     // {
//     //     SeparatedList {
//     //         parser: self,
//     //         separator,
//     //     }
//     // }
//
//     // fn map<F, B, I>(self, f: F) -> Map<Self, F, I>
//     // where
//     //     F: Fn(Output) -> B,
//     // {
//     //     Map {
//     //         parser: self,
//     //         f,
//     //         _marker: std::marker::PhantomData,
//     //     }
//     // }
// }
//
// pub struct Map<P, F, I> {
//     parser: P,
//     f: F,
//     _marker: std::marker::PhantomData<I>,
// }
//
// // impl<'a, TParser, TFunc, TInput, TOuput> ParseTokenStream<'a, TOuput> for Map<TParser, TFunc, TInput>
// // where
// //     TInput: 'a,
// //     TParser: ParseTokenStream<'a, TInput>,
// //     TFunc: Fn(TInput) -> TOuput,
// // {
// //     fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(TOuput, Span), TokenError> {
// //         let (output, span) = self.parser.parse(cursor)?;
// //         Ok(((self.f)(output), span))
// //     }
// // }
// //
// // /// Zero or more combinator
// // pub struct Many0<P> {
// //     parser: P,
// // }
// //
// // impl<'a, P, O> ParseTokenStream<'a, Vec<O>> for Many0<P>
// // where
// //     P: ParseTokenStream<'a, O>,
// // {
// //     fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(Vec<O>, Span), TokenError> {
// //         let mut items = Vec::new();
// //         let mut span = Span::default();
// //
// //         while let Ok((item, item_span)) = self.parser.parse(cursor) {
// //             items.push(item);
// //             span = span.merge(item_span);
// //         }
// //
// //         Ok((items, span))
// //     }
// // }
// //
// // /// One or more combinator
// // pub struct Many1<P> {
// //     parser: P,
// // }
// //
// // impl<'a, P, O> ParseTokenStream<'a, Vec<O>> for Many1<P>
// // where
// //     P: ParseTokenStream<'a, O>,
// // {
// //     fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(Vec<O>, Span), TokenError> {
// //         let (first, mut span) = self.parser.parse(cursor)?;
// //         let mut items = vec![first];
// //
// //         while let Ok((item, item_span)) = self.parser.parse(cursor) {
// //             items.push(item);
// //             span = span.merge(item_span);
// //         }
// //
// //         Ok((items, span))
// //     }
// // }
// //
// // pub struct Optional<P> {
// //     parser: P,
// // }
// //
// // impl<'a, O, P> ParseTokenStream<'a, Option<O>> for Optional<P>
// // where
// //     P: ParseTokenStream<'a, O>,
// // {
// //     fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(Option<O>, Span), TokenError> {
// //         // let checkpoint = cursor.checkpoint();
// //         // match self.parser.parse(cursor) {
// //         //     Ok((value, span)) => Ok((Some(value), span)),
// //         //     Err(_) => {
// //         //         cursor.restore(checkpoint);
// //         //         Ok((None, Span::default()))
// //         //     }
// //         // }
// //         let checkpoint = cursor.checkpoint();
// //         match self.parser.parse(cursor) {
// //             Ok((value, span)) => Ok((Some(value), span)),
// //             Err(_) => {
// //                 cursor.restore(checkpoint);
// //                 Ok((None, cursor.span()))
// //             }
// //         }
// //     }
// // }
// // pub struct SeparatedList<P, S> {
// //     parser: P,
// //     separator: S,
// // }
// //
// // impl<'a, P, S, O> ParseTokenStream<'a, Vec<O>> for SeparatedList<P, S>
// // where
// //     P: ParseTokenStream<'a, O>,
// //     S: ParseTokenStream<'a, ()>,
// // {
// //     fn parse(&self, cursor: &mut TokenStream<'a>) -> Result<(Vec<O>, Span), TokenError> {
// //         let mut items = Vec::new();
// //         let mut span = Span::default();
// //         let checkpoint = cursor.checkpoint();
// //
// //         match self.parser.parse(cursor) {
// //             Ok((first, first_span)) => {
// //                 items.push(first);
// //                 span = first_span;
// //
// //                 loop {
// //                     let sep_check = cursor.checkpoint();
// //                     match self.separator.parse(cursor) {
// //                         Ok((_, sep_span)) => {
// //                             span = span.merge(sep_span);
// //                             match self.parser.parse(cursor) {
// //                                 Ok((item, item_span)) => {
// //                                     items.push(item);
// //                                     span = span.merge(item_span);
// //                                 }
// //                                 Err(_) => {
// //                                     cursor.restore(sep_check);
// //                                     break;
// //                                 }
// //                             }
// //                         }
// //                         Err(_) => break,
// //                     }
// //                 }
// //                 Ok((items, span))
// //             }
// //             Err(_) => {
// //                 cursor.restore(checkpoint);
// //                 Ok((vec![], Span::default()))
// //             }
// //         }
// //     }
// // }
// //
// // // struct Ident(String, Span);
// // // /// Parse comma-separated identifiers
// // // fn identifier_list(cursor: &mut TokenStream) -> Result<Vec<Ident>, TokenError> {
// // //     let ident_parser = |c: &mut TokenStream| {
// // //         c.expect_token(TokenKind::Ident(String::new()))
// // //             .and_then(|t| match &t.kind {
// // //                 // TokenKind::Ident(name) => Ok(AstNode::identifier(name.clone(), t.span)),
// // //                 TokenKind::Ident(name) => Ok(Ident(name.clone(), t.span)),
// // //                 _ => Err(TokenError::UnexpectedToken {
// // //                     expected: "identifier".into(),
// // //                     found: format!("{:?}", t.kind),
// // //                     span: t.span,
// // //                 }),
// // //             })
// // //     };
// // //
// // //     let sep_parser = |c: &mut TokenStream| {
// // //         c.expect_token(TokenKind::Comma).map(|_| ())
// // //     };
// // //
// // //     ident_parser
// // //         .separated_list(sep_parser)
// // //         .parse(cursor)
// // //         .map(|(nodes, _)| nodes)
// // // }
// // //
// //
