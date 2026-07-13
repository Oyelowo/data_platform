
// #[derive(Debug, Clone, PartialEq)]
// pub enum MutationKind {
//     Create,
//     Upsert,
// }
//
// #[derive(Debug, Clone, PartialEq)]
// pub struct InsertQ {
//     pub kind: MutationKind,
//     pub label: Option<Ident>,
//     pub table: Type, // Supports `User` or `User:123`
//     pub data: CreationData,
//     pub links: Vec<CreatePath>,
//     pub return_: Option<Expr>,
//     pub span: Span,
// }
//
// impl ParseTokenStream for InsertQ {
//     fn parse(stream: &mut TokenStream) -> TokenResult<Self> {
//         let checkpoint = stream.checkpoint();
//
//         // 1. Kind
//         let kind = match_map!(
//             stream,
//             T![create] => |_| MutationKind::Create,
//             T![upsert] => |_| MutationKind::Upsert,
//         )?;
//
//         // 2. Header: `label? : Table`
//         let (label, _, table) = stream.parse::<(Option<Ident>, T![:], Type)>()?;
//
//         // 3. Data: Object or Array
//         let data = match_map!(
//             stream,
//             Object => CreationData::Object,
//             Array => CreationData::Array
//         )?;
//
//         // 4. Optional Links
//         let links = stream
//             .parse::<Option<(T![link], SeparatedList<CreatePath, T![,], true>)>>()?
//             .map(|(_, links)| links.value_owned())
//             .unwrap_or_default();
//
//         // 5. Return
//         let return_ = stream
//             .parse::<Option<(T![return], Expr)>>()?
//             .map(|(_, expr)| expr);
//
//         Ok(Self {
//             kind,
//             label,
//             table,
//             data,
//             links,
//             return_,
//             span: stream.span_since(checkpoint),
//         })
//     }
// }
//
// impl InsertQ {
//     pub fn span(&self) -> Span {
//         self.span
//     }
// }
