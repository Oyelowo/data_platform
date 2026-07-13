/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::{PeekNot, SeparatedList, SurroundedBy, Verify};
use crate::{
    CharCursor, CharLexerResult, ParseChars, ParseTokenStream, TokenResult, TokenStream, TokenTrait,
};
use std::marker::PhantomData;

// IMPERATIVE +  COMBINATOR PATTERN
/// ArrayCreator is a helper type that represents an array of items separated by a separator
/// and surrounded by a left and right delimiter.
/// It is used to parse arrays of items in a token stream.
///
/// # Example
/// ```rs
/// use crate::helper_types::array::ArrayCreator;
/// use crate::{TokenStream, TokenTrait, TokenResult, ParseTokenStream};
///
/// type Array = ArrayCreator<T!['['], T![i32], T![,], T![']']>;
/// ```
pub struct ArrayCreator<LDelim, TItem, Sep, RDelim> {
    items: Vec<TItem>,
    sep: PhantomData<Sep>,
    sep_count: usize,
    ldelim: LDelim,
    rdelim: RDelim,
}

impl<LDelim, TItem, Sep, RDelim> IntoIterator for ArrayCreator<LDelim, TItem, Sep, RDelim> {
    type Item = TItem;
    type IntoIter = std::vec::IntoIter<TItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, LDelim, TItem, Sep, RDelim> IntoIterator for &'a ArrayCreator<LDelim, TItem, Sep, RDelim> {
    type Item = &'a TItem;
    type IntoIter = std::slice::Iter<'a, TItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<LDelim, TItem, Sep, RDelim> ArrayCreator<LDelim, TItem, Sep, RDelim> {
    pub fn iter(&self) -> std::slice::Iter<'_, TItem> {
        self.items.iter()
    }

    pub fn items(&self) -> &Vec<TItem> {
        &self.items
    }

    pub fn items_owned(self) -> Vec<TItem> {
        self.items
    }

    // pub fn sep(&self) -> &Sep {
    //     &self.sep
    // }

    pub fn ldelim(&self) -> &LDelim {
        &self.ldelim
    }

    pub fn rdelim(&self) -> &RDelim {
        &self.rdelim
    }
}

impl<LDelim, TItem, Sep, RDelim, TKind> ParseTokenStream<TKind>
    for ArrayCreator<LDelim, TItem, Sep, RDelim>
where
    TKind: TokenTrait,
    TItem: ParseTokenStream<TKind>,
    Sep: ParseTokenStream<TKind>,
    LDelim: ParseTokenStream<TKind>,
    RDelim: ParseTokenStream<TKind>,
{
    fn parse(stream: &mut TokenStream<TKind>) -> TokenResult<Self> {
        let ldelim = stream.parse::<LDelim>()?;
        let mut sep_count = 0;

        let mut elements = Vec::new();
        while !stream.is_eof() {
            if stream.parse::<Verify<RDelim>>().is_ok() {
                break;
            }

            let element = stream.parse::<TItem>()?;
            elements.push(element);
            if stream.parse::<Sep>().is_err() {
                break;
            }
            sep_count += 1;
        }

        let rdelim = stream.parse::<RDelim>()?;

        Ok(ArrayCreator {
            items: elements,
            sep: PhantomData,
            sep_count,
            ldelim,
            rdelim,
        })
    }
}

// COMPLETE COMBINATOR PATTERN
// Achieves same result as the above code snippet but completely uses Combinator pattern
pub type Item<TItem, RDelim> = (Verify<PeekNot<RDelim>>, TItem);

pub type Content<TItem, RDelim, Sep, const ALLOW_TRAILING: bool> =
    SeparatedList<Item<TItem, RDelim>, Sep, ALLOW_TRAILING>;
impl<TItem, RDelim, Sep, const ALLOW_TRAILING: bool> Content<TItem, RDelim, Sep, ALLOW_TRAILING> {}

pub type List<TItem, Sep, LDelim, RDelim, const ALLOW_TRAILING: bool> =
    SurroundedBy<LDelim, (Content<TItem, RDelim, Sep, ALLOW_TRAILING>, Option<Sep>), RDelim>;

impl<TItem, Sep, LDelim, RDelim, const ALLOW_TRAILING: bool>
    List<TItem, Sep, LDelim, RDelim, ALLOW_TRAILING>
{
    pub fn content_tuple(&self) -> &(Content<TItem, RDelim, Sep, ALLOW_TRAILING>, Option<Sep>) {
        self.content()
    }

    pub fn items_owned(self) -> Vec<TItem> {
        self.content_owned()
            .0
            .items()
            .into_iter()
            .map(|x| x.1)
            .collect()
    }

    pub fn trailing_sep(&self) -> &Option<Sep> {
        &self.content().1
    }

    pub fn items(self) -> Vec<TItem> {
        let items_vec = self.content_owned().0.value_owned();

        items_vec.into_iter().map(|x| x.1).collect()
    }
}

// TODO: Consider removing these as their non-explicitness in terms of minimum token parsed
// may lead to very terrible subtle stackoverflow which I spent a better part of my week debugging
// myself even as the original author of this library.
// Prefer RepeatMin, RepeatMinMax, as those are explicit in terms of minimum token expected to be
// parsed. Vec<T> will happily parse zero tokens which may lead to unintended bugs especially
// when parse matching/attempting multiple tokens to return the first valid. This may come
// earlier and silently return Ok despite fail to parse anything, preventing the subsequent
// paths/alternatives to be explored
// Solves different problem by just getting same items repeatedly but can probably be extended to solve the above problem
// Just for vectors
// impl<T: ParseChars> ParseChars for Vec<T> {
//     fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
//         let mut results = Vec::new();
//         while let Ok(item) = cursor.parse::<T>() {
//             results.push(item);
//         }
//         Ok(results)
//     }
// }
//
// impl<TKind, T> ParseTokenStream<TKind> for Vec<T>
// where
//     TKind: TokenTrait,
//     T: ParseTokenStream<TKind>,
// {
//     fn parse(tokenstream: &mut TokenStream<TKind>) -> TokenResult<Self> {
//         let mut results = Vec::new();
//         while let Ok(item) = tokenstream.parse::<T>() {
//             results.push(item);
//         }
//         Ok(results)
//     }
// }
