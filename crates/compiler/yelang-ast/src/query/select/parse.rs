/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use super::*;
use crate::expr::Object;
use crate::{Expr, Ident, Precedence, Restrictions, T, Type};
use yelang_lexer::{
    Either, ParseTokenStream, SeparatedList, TokenError, TokenResult, TokenStream, match_map,
};

fn parse_optional_links_match_kind(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> LinksMatchKind {
    let Some(token) = stream.peek() else {
        return LinksMatchKind::Optional;
    };

    let crate::tokenizer::tokens::TokenKind::Ident(ident) = token.kind() else {
        return LinksMatchKind::Optional;
    };

    if ident.as_str(stream.interner()) != "inner" {
        return LinksMatchKind::Optional;
    }

    stream.advance();
    LinksMatchKind::Required
}

fn reject_legacy_range_tail(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<()> {
    if stream.peek().is_some_and(|t| {
        matches!(
            t.kind(),
            crate::tokenizer::tokens::TokenKind::Limit | crate::tokenizer::tokens::TokenKind::Start
        )
    }) {
        let span = stream.peek().map(|t| t.span()).unwrap_or_default();
        return Err(TokenError::CustomError {
            msg: "old SELECT `start` / `limit` clauses are not accepted; use `range start..end`"
                .to_string(),
            span,
        });
    }

    Ok(())
}

fn next_token_starts_link_segment(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> bool {
    stream.peek().is_some_and(|t| {
        matches!(
            t.kind(),
            crate::tokenizer::tokens::TokenKind::ArrowRight
                | crate::tokenizer::tokens::TokenKind::ArrowLeft
                | crate::tokenizer::tokens::TokenKind::ArrowBoth
        )
    })
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for HopRange {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // IMPORTANT: we must NOT parse `1..3` or `..3` as `ExprKind::Range` here.
        // In LINKS hop ranges, `..`/`..=` are delimiters.

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

        // Prefix form: `..end` / `..=end`
        let op_checkpoint = stream.checkpoint();
        if let Ok(op) = stream.parse::<Either<T![..=], T![..]>>() {
            let end = try_parse_bound(stream)?;
            return Ok(HopRange {
                start: None,
                end,
                inclusive: op.is_left(),
            });
        }
        stream.restore(op_checkpoint);

        // Otherwise: `start..end` / `start..=end`
        let start = try_parse_bound(stream)?;
        let op = stream.parse::<Either<T![..=], T![..]>>()?;
        let end = try_parse_bound(stream)?;

        Ok(HopRange {
            start,
            end,
            inclusive: op.is_left(),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for SelectQ {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        stream.parse::<T![select]>()?;
        let projection = stream.parse::<Expr>()?;

        if !stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::From_))
        {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(yelang_lexer::TokenError::CustomError {
                msg: "SELECT requires a `from` clause; rootless SELECT is not part of YeLang semantics"
                    .to_string(),
                span,
            });
        }

        stream.parse::<T![from]>()?;
        let from = stream
            .parse::<SeparatedList<FromNode, T![,], true>>()?
            .value_owned();

        let (links_match_kind, links) = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::Links))
        {
            let links = stream.parse::<LinksClause>()?;
            (links.match_kind, links.paths)
        } else {
            (LinksMatchKind::Optional, Vec::new())
        };

        // Parse per-root *post-LINKS* modifiers in multi-root SELECT.
        let mut post_links_for: Vec<ForRootModifiers> = Vec::new();
        let mut targeted_roots = rustc_hash::FxHashSet::default();
        while stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::For))
        {
            if from.len() == 1 {
                let span = stream.peek().map(|t| t.span()).unwrap_or_default();
                return Err(yelang_lexer::TokenError::CustomError {
                    msg: "`for <root> { ... }` blocks are only valid in multi-root SELECT; for single-root queries, use top-level `where` / `order by` / `range`".to_string(),
                    span,
                });
            }

            let (_, target, _, modifiers, _) =
                stream.parse::<(T![for], Ident, T!['{'], Modifiers, T!['}'])>()?;

            if !targeted_roots.insert(target.symbol) {
                return Err(yelang_lexer::TokenError::CustomError {
                    msg: "duplicate `for` block for the same root; each FROM root may be targeted at most once".to_string(),
                    span: target.span,
                });
            }

            // The target is validated during type-checking so that diagnostics
            // are produced through the same pipeline as other query errors.
            post_links_for.push(ForRootModifiers { target, modifiers });
        }

        // Multi-root SELECT has no single implicit stream for most tail clauses.
        if from.len() > 1 {
            if stream.peek().is_some_and(|t| {
                matches!(
                    t.kind(),
                    crate::tokenizer::tokens::TokenKind::Where
                        | crate::tokenizer::tokens::TokenKind::Order
                        | crate::tokenizer::tokens::TokenKind::RangeKw
                        | crate::tokenizer::tokens::TokenKind::Limit
                        | crate::tokenizer::tokens::TokenKind::Start
                )
            }) {
                let span = stream.peek().map(|t| t.span()).unwrap_or_default();
                return Err(yelang_lexer::TokenError::CustomError {
                    msg: "multi-root SELECT does not allow top-level tail clauses (`where`, `order by`, `range`) because there is no single implicit stream; use `for <root> { ... }` after `links` to attach tail stages to a specific root".to_string(),
                    span,
                });
            }
        }

        let where_clause = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::Where))
        {
            Some(stream.parse::<(T![where], Expr)>()?.1)
        } else {
            None
        };

        let group_by = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::Group))
        {
            Some(stream.parse::<GroupByClause>()?)
        } else {
            None
        };

        let order_by = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::Order))
        {
            let (_, _, parts) =
                stream.parse::<(T![order], T![by], SeparatedList<OrderByPart, T![,], true>)>()?;
            Some(parts.value_owned())
        } else {
            None
        };

        let range = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::RangeKw))
        {
            Some(stream.parse::<Range>()?)
        } else {
            None
        };

        reject_legacy_range_tail(stream)?;

        // Helpful diagnostic: `for` blocks must appear after optional `links` and before tail.
        if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::For))
        {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(yelang_lexer::TokenError::CustomError {
                msg: "`for <root> { ... }` blocks must appear after the optional `links` clause and before any top-level tail clauses.".to_string(),
                span,
            });
        }

        // Helpful diagnostic: `links` must appear immediately after FROM list.
        if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::Links))
        {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(yelang_lexer::TokenError::CustomError {
                msg: "`links` must come immediately after the `from` list in a SELECT query. If you used `for <root> { ... }` blocks, place them after `links`. If you intended to use FROM modifiers, wrap the FROM source in parentheses: `from (users@u:User where ...) links ...` (FROM modifiers require parentheses).".to_string(),
                span,
            });
        }

        Ok(Self {
            projection,
            from,
            links_match_kind,
            links,
            post_links_for,
            where_clause,
            group_by,
            order_by,
            range,
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Direction {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let direction = match_map!(
            stream,
            T![->] => |_| Direction::Outgoing,
            T![<-] => |_| Direction::Incoming,
        )?;
        Ok(direction)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LinksClause {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type MultiPaths = SeparatedList<LinkPath, T![,], true>;
        stream.parse::<T![links]>()?;
        let match_kind = parse_optional_links_match_kind(stream);
        let paths = stream.parse::<MultiPaths>()?.value_owned();
        let _trailing_comma = stream.parse::<Option<T![,]>>()?;

        Ok(LinksClause { match_kind, paths })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for EdgeDirection {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        match_map!(
            stream,
            T![->] => |_| EdgeDirection::Forward,
            T![<-] => |_| EdgeDirection::Backward,
            T!["<->"] => |_| EdgeDirection::Bidirectional,
        )
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for SortDirection {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        match_map!(
            stream,
            T![asc] => |_| SortDirection::Asc,
            T![desc] => |_| SortDirection::Desc,
        )
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for OrderByPart {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let field = stream.parse::<Expr>()?;
        let direction = stream.parse::<Option<SortDirection>>()?;
        Ok(Self {
            field,
            direction: direction.unwrap_or(SortDirection::Asc),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for OrderByClause {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_, _, orders) =
            stream.parse::<(T![order], T![by], SeparatedList<OrderByPart, T![,], true>)>()?;

        Ok(Self {
            orders: orders.value_owned(),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Range {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        stream.parse::<T![range]>()?;

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

        let sep_checkpoint = stream.checkpoint();
        if let Ok(sep) = stream.parse::<Sep>() {
            let end = try_parse_bound(stream)?;
            if end.is_none() {
                return Err(TokenError::CustomError {
                    msg: "`range ..` is empty; use `range ..end` or `range start..`".to_string(),
                    span: stream.span_since(checkpoint),
                });
            }
            return Ok(Self {
                start: None,
                end,
                inclusive: sep.is_left(),
            });
        }
        stream.restore(sep_checkpoint);

        let start = try_parse_bound(stream)?;
        let sep = stream.parse::<Sep>()?;
        let end = try_parse_bound(stream)?;

        Ok(Self {
            start,
            end,
            inclusive: sep.is_left(),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Modifiers {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (w, o, r) = stream.parse::<(
            Option<(T![where], Expr)>,
            Option<(T![order], T![by], SeparatedList<OrderByPart, T![,], true>)>,
            Option<Range>,
        )>()?;

        Ok(Self {
            filter: w.map(|(_, e)| e),
            order: o.map(|(_, _, l)| l.value_owned()),
            range: r,
        })
    }
}

type EntityHead = (
    Option<Ident>,          // var
    Option<(T![@], Ident)>, // bind
    Option<(T![:], Type)>,  // type
);

impl ParseTokenStream<crate::tokenizer::TokenKind> for FromNode {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::OpenParen))
        {
            let (_, (var, bind, ty), modifiers, _) =
                stream.parse::<(T!['('], EntityHead, Modifiers, T![')'])>()?;
            return Ok(Self {
                var,
                bind: bind.map(|(_, b)| b),
                ty: ty.map(|(_, t)| t),
                modifiers,
            });
        }

        let (var, bind, ty) = stream.parse::<EntityHead>()?;

        Ok(Self {
            var,
            bind: bind.map(|(_, b)| b),
            ty: ty.map(|(_, t)| t),
            modifiers: Modifiers::default(),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Node {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_, (var, bind, ty), modifiers, _) =
            stream.parse::<(T!['('], EntityHead, Modifiers, T![')'])>()?;

        Ok(Self {
            var,
            bind: bind.map(|(_, b)| b),
            ty: ty.map(|(_, t)| t),
            modifiers,
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Edge {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        stream.parse::<T!['[']>()?;
        let (var, bind, ty) = stream.parse::<EntityHead>()?;
        let hops = stream.parse::<Option<(T![hops], HopRange)>>()?;
        let modifiers = stream.parse::<Modifiers>()?;

        if !stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::CloseBracket))
        {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(TokenError::CustomError {
                msg: "unexpected token in LINKS edge; hop ranges must use `hops start..end`, and the only edge clauses after the edge head are `hops`, `where`, `order by`, and `range`".to_string(),
                span,
            });
        }

        stream.parse::<T![']']>()?;

        Ok(Self {
            var,
            bind: bind.map(|(_, b)| b),
            ty: ty.map(|(_, t)| t),
            hops: hops.map(|(_, range)| range),
            modifiers,
            direction: EdgeDirection::Forward,
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LinkSegment {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let direction = stream.parse::<EdgeDirection>()?;

        let mut edge = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::OpenBracket))
        {
            stream.parse::<Edge>()?
        } else {
            Edge {
                var: None,
                bind: None,
                ty: None,
                hops: None,
                modifiers: Modifiers::default(),
                direction: EdgeDirection::Forward,
            }
        };

        edge.direction = direction;

        // The target node is preceded by the same direction arrow (e.g.
        // `->[edge]->(target)` or `<-[edge]<-(target)`).
        let _target_direction = stream.parse::<EdgeDirection>()?;

        let target = if stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::OpenParen))
        {
            stream.parse::<Node>()?
        } else {
            Node {
                var: None,
                bind: None,
                ty: None,
                modifiers: Modifiers::default(),
            }
        };

        Ok(Self { edge, target })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for LinkPath {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let start = stream.parse::<Node>()?;

        if !next_token_starts_link_segment(stream) {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(TokenError::CustomError {
                msg: "LINKS path requires at least one edge segment after the start node"
                    .to_string(),
                span,
            });
        }

        let mut segments = Vec::new();
        while next_token_starts_link_segment(stream) {
            segments.push(stream.parse::<LinkSegment>()?);
        }

        Ok(Self { start, segments })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GroupByKey {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let parsed = stream.parse::<Either<(Ident, T![:], Expr), Expr>>()?;
        Ok(match parsed {
            Either::Left((name, _, expr)) => Self {
                name: Some(name),
                expr,
            },
            Either::Right(expr) => Self { name: None, expr },
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GroupByClause {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        stream.parse::<T![group]>()?;
        stream.parse::<T![by]>()?;

        if !stream
            .peek()
            .is_some_and(|t| matches!(t.kind(), crate::tokenizer::tokens::TokenKind::OpenBrace))
        {
            let span = stream.peek().map(|t| t.span()).unwrap_or_default();
            return Err(TokenError::CustomError {
                msg: "GROUP BY requires object keys: `group by { key: <expr>, ... } into <label>`"
                    .to_string(),
                span,
            });
        }

        let obj = stream.parse::<Object>()?;
        let keys: Vec<GroupByKey> = obj
            .fields
            .into_iter()
            .map(|f| GroupByKey {
                name: Some(f.key),
                expr: f.val,
            })
            .collect();

        let into = stream
            .parse::<Option<(T![into], Ident)>>()?
            .map(|(_, ident)| ident);

        let Some(into) = into else {
            let span = keys.last().map(|k| k.expr.span).unwrap_or(obj.span);
            return Err(TokenError::CustomError {
                msg: "GROUP BY requires an `into <label>` target. Example: `group by { age: u.age } into groups`".to_string(),
                span,
            });
        };

        Ok(Self { keys, into })
    }
}
