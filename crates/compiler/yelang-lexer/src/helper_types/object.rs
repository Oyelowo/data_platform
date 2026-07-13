/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 02/10/2025
 */
use super::Verify;
use crate::{ParseTokenStream, TokenResult, TokenStream, TokenTrait};
use std::marker::PhantomData;

/// A helper type for creating objects
/// All types must implement the ParseTokenStream trait
///
/// # Example
///
/// ```rust, ignore
/// let input = r#"{ "name": "John", "age": 20 }"#;
/// let mut stream = TokenStream::new(input);
/// let obj = stream.parse::<ObjectCreator<
///                     OpenBrace,
///                     Key,
///                     Colon,
///                     Value,
///                     Comma,
///                     CloseBrace
///                 >>();
///
/// let (obj, span) = stream.parse_with_span::<ObjectCreator<
///                                 T!["{"],
///                                 Ident,
///                                 T![":"],
///                                 Value,
///                                 T![","],
///                                 T!["}"],
///                             >>()?;
///
/// assert_eq!(obj.items().len(), 2);
/// ```
pub struct ObjectCreator<LDelim, K, KVSep, V, Sep, RDelim> {
    items: Vec<(K, V)>,
    kvs_sep: PhantomData<KVSep>,
    sep: PhantomData<Sep>,
    sep_count: usize,
    ldelim: LDelim,
    rdelim: RDelim,
}

impl<LDelim, K, KVSep, V, Sep, RDelim> ObjectCreator<LDelim, K, KVSep, V, Sep, RDelim> {
    pub fn new(items: Vec<(K, V)>, ldelim: LDelim, rdelim: RDelim) -> Self {
        ObjectCreator {
            items,
            kvs_sep: PhantomData,
            sep: PhantomData,
            sep_count: 0,
            ldelim,
            rdelim,
        }
    }

    pub fn items(&self) -> &Vec<(K, V)> {
        &self.items
    }

    pub fn items_owned(self) -> Vec<(K, V)> {
        self.items
    }

    pub fn ldelim(&self) -> &LDelim {
        &self.ldelim
    }

    pub fn rdelim(&self) -> &RDelim {
        &self.rdelim
    }
}

impl<LDelim, K, KVSep, V, Sep, RDelim, TKind> ParseTokenStream<TKind>
    for ObjectCreator<LDelim, K, KVSep, V, Sep, RDelim>
where
    LDelim: ParseTokenStream<TKind>,
    K: ParseTokenStream<TKind>,
    KVSep: ParseTokenStream<TKind>,
    V: ParseTokenStream<TKind>,
    Sep: ParseTokenStream<TKind>,
    RDelim: ParseTokenStream<TKind>,
    TKind: TokenTrait,
{
    fn parse(stream: &mut TokenStream<TKind>) -> TokenResult<Self> {
        let ldelim = stream.parse::<LDelim>()?;
        let mut sep_count = 0;

        let mut elements = Vec::new();
        while !stream.is_eof() {
            if stream.parse::<Verify<RDelim>>().is_ok() {
                break;
            }

            let key = stream.parse::<K>()?;
            stream.parse::<KVSep>()?;
            let value = stream.parse::<V>()?;
            elements.push((key, value));
            if stream.parse::<Sep>().is_err() {
                break;
            }
            sep_count += 1;
        }

        let rdelim = stream.parse::<RDelim>()?;

        Ok(ObjectCreator {
            items: elements,
            kvs_sep: PhantomData,
            sep: PhantomData,
            sep_count,
            ldelim,
            rdelim,
        })
    }
}
