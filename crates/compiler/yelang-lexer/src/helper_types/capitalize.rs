//
// pub struct Capitalize<T>(pub T);
//
// impl<T> ParseChars for Capitalize<T>
// where
//     T: ParseChars,
// {
//     fn parse(cursor: &mut CharCursor) -> CharLexerResult<Self> {
//         let inner = cursor.parse::<T>()?;
//         Ok(Capitalize(inner))
//     }
// }
//
// impl<T, To> ParseTokenStream<To> for Capitalize<T>
// where
//     To: TokenTrait,
//     T: ParseTokenStream<To>,
// {
//     fn parse(tokenstream: &mut TokenStream<To>) -> TokenResult<Self> {
//         let inner = tokenstream.parse::<T>()?;
//         Ok(Capitalize(inner))
//     }
// }
