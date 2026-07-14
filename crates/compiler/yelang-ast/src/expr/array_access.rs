/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/02/2025
 */
use crate::{Expr, Ident, Object, OrderByClause, Precedence, Restrictions, T};
use std::fmt::Display;
use yelang_lexer::{Either, ParseTokenStream, RepeatMin, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayAccess {
    pub base: Box<Expr>,
    pub index: ArrayIndex,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ArrayIndex {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        stream.parse::<T!['[']>()?;

        // Star-based selectors:
        // - [*]      => Stars { stars: 1 }   (iterate/map)
        // - [**..]   => Stars { stars: N }   (flatten depth is typically stars - 1)
        let res = match_map!(
            stream,
            (T![group], T![by], GroupBySelector) => |(_, _, selector)| {
                ArrayIndex::GroupBy(selector)
            },
            (T![distinct], T![by], Expr) => |(_, _, expr)| {
                ArrayIndex::DistinctBy(Box::new(expr))
            },
            T![distinct] => |_| ArrayIndex::Distinct,
            T![enumerate] => |_| ArrayIndex::Enumerate,
            RepeatMin<1, T![*]> => |stars| ArrayIndex::Stars {
                stars: stars.value_owned().len(),
            },
            RangeItem => ArrayIndex::Range,
            (T![where], Expr) => |(_, expr)| ArrayIndex::Filter(Box::new(expr)),
            OrderByClause => ArrayIndex::OrderBy,
            Expr => |this| ArrayIndex::Single(Index(Box::new(this))),
        )?;
        stream.parse::<T![']']>()?;

        Ok(res)
    }
}

impl ArrayAccess {
    pub fn base(&self) -> &Expr {
        &self.base
    }

    pub fn index(&self) -> &ArrayIndex {
        &self.index
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ArrayIndex {
    /// e.g. [0], [-1]
    Single(Index),
    /// Slice selector.
    ///
    /// Canonical syntax: `[start..end]`, e.g. `[5..8]`, `[-2..6]`, `[2..]`, `[..4]`, `[1..-2]`.
    Range(RangeItem),
    // e.g. [WHERE user.age>30]
    Filter(Box<Expr>),

    /// [*], [**], [***], ...
    ///
    /// The number of `*` tokens encountered inside the brackets.
    /// Semantics:
    /// - stars == 1 => iterate/map
    /// - stars >= 2 => flatten depth is typically `stars - 1`
    Stars {
        stars: usize,
    },
    /// e.g. [ORDER BY .priority DESC]
    // TODO: Rename to OrderBySpec?
    OrderBy(OrderByClause),
    /// e.g. [GROUP BY { city: user.city }]
    GroupBy(GroupBySelector),
    /// e.g. [ENUMERATE]
    Enumerate,
    /// e.g. [DISTINCT]
    Distinct,
    /// e.g. [DISTINCT BY user.id]
    DistinctBy(Box<Expr>),
    // Ternary(Box<Expr>),
}

impl ArrayIndex {
    pub fn is_single(&self) -> bool {
        matches!(self, ArrayIndex::Single(_))
    }

    pub fn is_range(&self) -> bool {
        matches!(self, ArrayIndex::Range(_))
    }

    pub fn is_filter(&self) -> bool {
        matches!(self, ArrayIndex::Filter(_))
    }

    pub fn is_star_selector(&self) -> bool {
        matches!(self, ArrayIndex::Stars { .. })
    }

    pub fn star_count(&self) -> Option<usize> {
        match self {
            ArrayIndex::Stars { stars } => Some(*stars),
            _ => None,
        }
    }

    pub fn is_iterate(&self) -> bool {
        matches!(self, ArrayIndex::Stars { stars: 1 })
    }

    pub fn is_flatten(&self) -> bool {
        matches!(self, ArrayIndex::Stars { stars } if *stars >= 2)
    }

    pub fn is_order_by(&self) -> bool {
        matches!(self, ArrayIndex::OrderBy(_))
    }

    pub fn is_group_by(&self) -> bool {
        matches!(self, ArrayIndex::GroupBy(_))
    }

    pub fn is_enumerate(&self) -> bool {
        matches!(self, ArrayIndex::Enumerate)
    }

    pub fn is_distinct(&self) -> bool {
        matches!(self, ArrayIndex::Distinct | ArrayIndex::DistinctBy(_))
    }
}

impl ArrayIndex {
    fn display(&self) -> String {
        match self {
            ArrayIndex::Single(index) => format!("[{}]", "index"),
            ArrayIndex::Range(range) => {
                format!("[{}]", range)
            }
            ArrayIndex::Stars { stars } => format!("[{}]", "*".repeat(*stars)),
            ArrayIndex::Filter(expr) => format!("[WHERE {}]", "expr"),
            ArrayIndex::OrderBy(_) => format!("[ORDER BY ...]"),
            ArrayIndex::GroupBy(_) => format!("[GROUP BY ...]"),
            ArrayIndex::Enumerate => "[ENUMERATE]".to_string(),
            ArrayIndex::Distinct => "[DISTINCT]".to_string(),
            ArrayIndex::DistinctBy(_) => "[DISTINCT BY ...]".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupBySelector {
    pub keys: Vec<GroupBySelectorKey>,
}

impl GroupBySelector {
    pub fn keys(&self) -> &[GroupBySelectorKey] {
        &self.keys
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupBySelectorKey {
    pub name: Ident,
    pub expr: Expr,
}

impl GroupBySelectorKey {
    pub fn name(&self) -> &Ident {
        &self.name
    }

    pub fn expr(&self) -> &Expr {
        &self.expr
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GroupBySelector {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let obj = stream.parse::<Object>()?;
        let keys = obj
            .fields
            .into_iter()
            .map(|field| GroupBySelectorKey {
                name: field.key,
                expr: field.val,
            })
            .collect();

        Ok(Self { keys })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Index(pub Box<Expr>);

impl Index {
    pub fn expr(&self) -> &Expr {
        &self.0
    }
}

// impl ParseTokenStream<Token> for Index {
//     fn parse(stream: &mut TokenStream<Token>) -> TokenResult<Self> {
//         let res = stream.parse::<(T!['['], Expr, T![']'])>()?;
//         Ok(Self(Box::new(res.1)))
//     }
// }

#[derive(Debug, Clone, PartialEq)]
pub struct RangeItem {
    pub start: Option<Box<Expr>>,
    pub end: Option<Box<Expr>>,
    pub inclusive: bool,
}

impl RangeItem {
    pub fn start(&self) -> Option<&Box<Expr>> {
        self.start.as_ref()
    }

    fn end(&self) -> Option<&Box<Expr>> {
        self.end.as_ref()
    }
}

impl Display for RangeItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let val = |i: Option<&Box<Expr>>| i.map_or_else(|| "".into(), |i| "expr");

        let start = val(self.start.as_ref());
        let end = val(self.end.as_ref());
        if self.inclusive {
            write!(f, "{}..={}", start, end)
        } else {
            write!(f, "{}..{}", start, end)
        }
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for RangeItem {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // IMPORTANT: we must NOT parse `1..3` or `..3` as `ExprKind::Range` here.
        // Inside an array selector, `..`/`..=` are delimiters for slices.
        // So we parse bounds with precedence strictly above `Range`.

        type Sep = Either<T![..=], T![..]>;

        let parse_bound_expr = |stream: &mut TokenStream<crate::tokenizer::TokenKind>| {
            Expr::parse_pratt(stream, Precedence::Range.increment(), Restrictions::NONE)
        };

        let try_parse_bound =
            |stream: &mut TokenStream<crate::tokenizer::TokenKind>| -> TokenResult<Option<Expr>> {
                let checkpoint = stream.checkpoint();
                match parse_bound_expr(stream) {
                    Ok(expr) => Ok(Some(expr)),
                    Err(_) => {
                        stream.restore(checkpoint);
                        Ok(None)
                    }
                }
            };

        // Prefix form: `[..end]` / `[..=end]`
        let sep_checkpoint = stream.checkpoint();
        if let Ok(sep) = stream.parse::<Sep>() {
            let inclusive = sep.is_left();
            let end = try_parse_bound(stream)?;
            return Ok(Self {
                start: None,
                end: end.map(Box::new),
                inclusive,
            });
        }
        stream.restore(sep_checkpoint);

        // Otherwise parse `start` first.
        let start = try_parse_bound(stream)?;
        let sep = stream.parse::<Sep>()?;
        let inclusive = sep.is_left();
        let end = try_parse_bound(stream)?;

        Ok(Self {
            start: start.map(Box::new),
            end: end.map(Box::new),
            inclusive,
        })
    }
}

// parse [stuff], e.g. [0], [2..6], [WHERE user.age>30], [*], or [ORDER BY ...]
// #[derive(Debug, Clone, PartialEq)]
// pub struct OrderSpec {
//     pub field: Expr,
//     pub direction: SortDirection,
// }
//
// // #[derive(Debug, Clone, PartialEq)]
// // pub enum ExpressionOrPath {
// //     // Expr(Expr),
// //     Path(PathExpr),
// // }
//
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum SortDirection {
//     Asc,
//     Desc,
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interner, TokenKind};
    use yelang_lexer::{Token, TokenStream, TokenizeChars};

    #[test]
    fn test_path_expr() {
        // let input = "user[0].settings[1..5].mode";
        let input = "user[0].settings[1..=5].mode[*].age";
        let mut interner = Interner::new();
        let mut tokens = TokenKind::tokenize(input, &mut interner).unwrap();
        // let mut tokens = Token::tokenize_with_interner(input).unwrap();
        // let ast = tokens
        //     .parse::<PathExpr>()
        //     .inspect(|t| {
        //         panic!("loowow{:#?}", t);
        //     })
        //     .inspect_err(|e| {
        //         panic!("errorrr{}", e);
        //     })
        //     .unwrap();
        // let ast = PathExpr::parse(&mut tokens).unwrap();
        // panic!("result = {}", ast);
    }

    #[test]
    fn test_array_index_multi_star_flatten_parses_star_count() {
        let mut interner = Interner::new();

        let mut tokens = TokenKind::tokenize("[**]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Stars { stars: 2 }));

        let mut tokens = TokenKind::tokenize("[***]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Stars { stars: 3 }));

        let mut tokens = TokenKind::tokenize("[*]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Stars { stars: 1 }));
    }

    #[test]
    fn test_array_index_slice_range_parses_dotdot() {
        let mut interner = Interner::new();

        let mut tokens = TokenKind::tokenize("[1..3]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Range(_)));

        let mut tokens = TokenKind::tokenize("[..3]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Range(_)));

        let mut tokens = TokenKind::tokenize("[1..]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Range(_)));

        let mut tokens = TokenKind::tokenize("[1..=3]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Range(_)));
    }

    #[test]
    fn test_array_index_order_by_parses_multiple_parts_with_commas() {
        let mut interner = Interner::new();

        let mut tokens = TokenKind::tokenize("[order by x desc, y]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();

        let ArrayIndex::OrderBy(clause) = idx else {
            panic!("expected ArrayIndex::OrderBy, got {idx:?}");
        };

        assert_eq!(clause.orders.len(), 2);
        assert_eq!(clause.orders[0].direction, crate::SortDirection::Desc);
        assert_eq!(clause.orders[1].direction, crate::SortDirection::Asc);
    }

    #[test]
    fn test_array_index_group_by_selector_parses_object_keys() {
        let mut interner = Interner::new();

        let mut tokens =
            TokenKind::tokenize("[group by { city: u.city, team: u.team }]", &mut interner)
                .unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();

        let ArrayIndex::GroupBy(selector) = idx else {
            panic!("expected ArrayIndex::GroupBy, got {idx:?}");
        };

        assert_eq!(selector.keys.len(), 2);
        assert_eq!(selector.keys[0].name.as_str(&interner), "city");
        assert_eq!(selector.keys[1].name.as_str(&interner), "team");
    }

    #[test]
    fn test_array_index_enumerate_and_distinct_selectors_parse() {
        let mut interner = Interner::new();

        let mut tokens = TokenKind::tokenize("[enumerate]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Enumerate));

        let mut tokens = TokenKind::tokenize("[distinct]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::Distinct));

        let mut tokens = TokenKind::tokenize("[distinct by u.id]", &mut interner).unwrap();
        let idx = tokens.parse::<ArrayIndex>().unwrap();
        assert!(matches!(idx, ArrayIndex::DistinctBy(_)));
    }
}

// #[derive(Debug, Clone)]
// pub enum FilterExpr {
//     /// A simple comparison like `user.age > 30`
//     Comparison {
//         left: &'a Ident,
//         op: ComparisonOp,
//         right: Value,
//     },
//     /// A logical combination (AND/OR) of filters.
//     Logical {
//         left: Box<FilterExpr>,
//         op: LogicalOp,
//         right: Box<FilterExpr>,
//     },
//     /// Parenthesized filter for grouping.
//     Group(Box<FilterExpr>),
// }

// #[derive(Debug, Clone, Copy)]
// pub enum ComparisonOp {
//     Eq,
//     NotEq,
//     Lt,
//     Lte,
//     Gt,
//     Gte,
// }
//
// #[derive(Debug, Clone, Copy)]
// pub enum LogicalOp {
//     And,
//     Or,
// }
//
// #[derive(Debug, Clone)]
// pub enum Expr {
//     Literal(Literal),
//     Path(PathExpr),
// }
//
// #[derive(Debug, Clone)]
// pub struct BoolExpr(pub InnerBoolExpr);
//
// #[derive(Debug, Clone)]
// enum InnerBoolExpr {
//     Comparison {
//         lhs: Expr,
//         op: ComparisonOp,
//         rhs: Expr,
//     },
//     Logical {
//         lhs: Box<BoolExpr>,
//         op: LogicalOp,
//         rhs: Box<BoolExpr>,
//     },
//     Grouped(Box<BoolExpr>),
//     Expr(Expr), // For boolean literals/paths
// }

// #[derive(Debug, Clone)]
// pub struct WhereFilter {
//     pub condition: BoolExpr,
// }

// #[derive(Debug, Clone)]
// pub struct PathFilter {
//     pub condition: FilterExpr,
// }
