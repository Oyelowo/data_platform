/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */

// use crate::{
//     CharCursor, CharLexerError, CharLexerResult, ParseChars, ParseTokenStream, Span, TokenResult,
//     TokenStream, TokenTrait,
// };
//
// #[derive(Debug, Clone, PartialEq)]
// pub struct And<A, B> {
//     pub a: A,
//     pub b: B,
// }
//
// impl<A, B> And<A, B> {
//     pub fn new(a: A, b: B) -> Self {
//         And { a, b }
//     }
//
//     pub fn a(&self) -> &A {
//         &self.a
//     }
//
//     pub fn b(&self) -> &B {
//         &self.b
//     }
//
//     pub fn a_mut(&mut self) -> &mut A {
//         &mut self.a
//     }
//
//     pub fn b_mut(&mut self) -> &mut B {
//         &mut self.b
//     }
//
//     pub fn a_owned(self) -> A {
//         self.a
//     }
//
//     pub fn b_owned(self) -> B {
//         self.b
//     }
//
//     pub fn into_parts(self) -> (A, B) {
//         (self.a, self.b)
//     }
//
//     pub fn map<C, D>(self, f: impl FnOnce(A, B) -> (C, D)) -> And<C, D> {
//         let (a, b) = f(self.a, self.b);
//         And::new(a, b)
//     }
//
//     pub fn map_a<C>(self, f: impl FnOnce(A) -> C) -> And<C, B> {
//         let a = f(self.a);
//         And::new(a, self.b)
//     }
//
//     pub fn map_b<D>(self, f: impl FnOnce(B) -> D) -> And<A, D> {
//         let b = f(self.b);
//         And::new(self.a, b)
//     }
//
//     pub fn map_ref<C, D>(&self, f: impl FnOnce(&A, &B) -> (C, D)) -> And<C, D> {
//         let (a, b) = f(&self.a, &self.b);
//         And::new(a, b)
//     }
//
//     pub fn map_mut<C, D>(&mut self, f: impl FnOnce(&mut A, &mut B) -> (C, D)) -> And<C, D> {
//         let (a, b) = f(&mut self.a, &mut self.b);
//         And::new(a, b)
//     }
// }
//
// impl<A, B, T: TokenTrait> ParseTokenStream<T> for And<A, B>
// where
//     A: ParseTokenStream<T>,
//     B: ParseTokenStream<T>,
// {
//     fn parse(stream: &mut TokenStream<T>) -> TokenResult<Self> {
//         let checkpoint = stream.checkpoint();
//
//         match (stream.parse::<A>(), stream.parse::<B>()) {
//             (Ok(a), Ok(b)) => Ok(And::new(a, b)),
//             (Err(e), _) | (_, Err(e)) => {
//                 stream.restore(checkpoint);
//                 Err(e)
//             }
//         }
//     }
// }
