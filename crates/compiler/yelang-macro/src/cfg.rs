//! Configuration predicate evaluation.
//!
//! Shared between the eager `cfg!` macro and item-level `#[cfg(...)]` /
//! `#[cfg_attr(...)]` attribute processing.

use std::collections::{HashMap, HashSet};

use yelang_ast::{Attribute, AttributeArgs, Codegen, Expr, ExprKind};
use yelang_interner::Interner;
use yelang_macro_core::token_tree::{Delimiter, LitKind, TokenStream, TokenTree};

use crate::eager::EagerContext;
use crate::eager::expand_eager_macros_in_stream;
use crate::error::ExpandError;

/// A predicate accepted by `cfg!` and `#[cfg]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgPredicate {
    Name(String),
    KeyValue(String, String),
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

/// Active `cfg` options used to evaluate predicates.
#[derive(Debug, Clone, Default)]
pub struct CfgOptions {
    pub names: HashSet<String>,
    pub key_values: HashMap<String, String>,
}

impl CfgOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.names.insert(name.into());
        self
    }

    pub fn with_key_value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.key_values.insert(key.into(), value.into());
        self
    }

    pub fn check(&self, predicate: &CfgPredicate) -> bool {
        match predicate {
            CfgPredicate::Name(name) => self.names.contains(name),
            CfgPredicate::KeyValue(key, value) => self
                .key_values
                .get(key)
                .map(|v| v == value)
                .unwrap_or(false),
            CfgPredicate::All(preds) => preds.iter().all(|p| self.check(p)),
            CfgPredicate::Any(preds) => preds.iter().any(|p| self.check(p)),
            CfgPredicate::Not(pred) => !self.check(pred),
        }
    }
}

/// Parse a `cfg` predicate from a token stream.
pub fn parse_cfg_predicate(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: yelang_lexer::Span,
) -> Result<CfgPredicate, ExpandError> {
    let expanded = expand_eager_macros_in_stream(args, ctx).map_err(|e| {
        ExpandError::malformed_macro_args(format!("cfg predicate expansion failed: {}", e), span)
    })?;
    let mut iter = expanded.iter().peekable();
    parse_cfg_predicate_from_iter(&mut iter, ctx, span)
}

fn parse_cfg_predicate_from_iter<'a>(
    iter: &mut std::iter::Peekable<impl Iterator<Item = &'a TokenTree>>,
    ctx: &EagerContext<'_>,
    span: yelang_lexer::Span,
) -> Result<CfgPredicate, ExpandError> {
    let tree = iter.next().ok_or_else(|| {
        ExpandError::malformed_macro_args("cfg predicate expected".to_string(), span)
    })?;

    match tree {
        TokenTree::Ident(ident) => {
            let name = ctx.interner.resolve(&ident.sym);
            match name {
                "all" | "any" => {
                    let open = iter.next().ok_or_else(|| {
                        ExpandError::malformed_macro_args(format!("{} expected `(`", name), span)
                    })?;
                    let TokenTree::Group(group) = open else {
                        return Err(ExpandError::malformed_macro_args(
                            format!("{} expected `(`", name),
                            span,
                        ));
                    };
                    if group.delimiter != Delimiter::Parenthesis {
                        return Err(ExpandError::malformed_macro_args(
                            format!("{} expected `(`", name),
                            span,
                        ));
                    }
                    let mut inner = group.stream.iter().peekable();
                    let mut preds = Vec::new();
                    while inner.peek().is_some() {
                        preds.push(parse_cfg_predicate_from_iter(&mut inner, ctx, span)?);
                        if let Some(comma) = inner.peek() {
                            if is_punct(comma, ',') {
                                inner.next();
                            } else {
                                return Err(ExpandError::malformed_macro_args(
                                    "expected `,` in cfg predicate".to_string(),
                                    span,
                                ));
                            }
                        }
                    }
                    if name == "all" {
                        Ok(CfgPredicate::All(preds))
                    } else {
                        Ok(CfgPredicate::Any(preds))
                    }
                }
                "not" => {
                    let open = iter.next().ok_or_else(|| {
                        ExpandError::malformed_macro_args("not expected `(`".to_string(), span)
                    })?;
                    let TokenTree::Group(group) = open else {
                        return Err(ExpandError::malformed_macro_args(
                            "not expected `(`".to_string(),
                            span,
                        ));
                    };
                    if group.delimiter != Delimiter::Parenthesis {
                        return Err(ExpandError::malformed_macro_args(
                            "not expected `(`".to_string(),
                            span,
                        ));
                    }
                    let mut inner = group.stream.iter().peekable();
                    let pred = parse_cfg_predicate_from_iter(&mut inner, ctx, span)?;
                    Ok(CfgPredicate::Not(Box::new(pred)))
                }
                _ => {
                    // `name` or `name = "value"`
                    if let Some(eq) = iter.peek()
                        && is_punct(eq, '=')
                    {
                        iter.next();
                        let value_tree = iter.next().ok_or_else(|| {
                            ExpandError::malformed_macro_args(
                                "cfg expected value after `=`".to_string(),
                                span,
                            )
                        })?;
                        let TokenTree::Literal(lit) = value_tree else {
                            return Err(ExpandError::malformed_macro_args(
                                "cfg value must be a string literal".to_string(),
                                span,
                            ));
                        };
                        let LitKind::Str { value, .. } = &lit.kind else {
                            return Err(ExpandError::malformed_macro_args(
                                "cfg value must be a string literal".to_string(),
                                span,
                            ));
                        };
                        let value = ctx.interner.resolve(value).to_string();
                        return Ok(CfgPredicate::KeyValue(name.to_string(), value));
                    }
                    Ok(CfgPredicate::Name(name.to_string()))
                }
            }
        }
        _ => Err(ExpandError::malformed_macro_args(
            "cfg predicate must start with an identifier".to_string(),
            span,
        )),
    }
}

fn is_punct(tree: &TokenTree, ch: char) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ch)
}

/// Evaluate a `#[cfg(predicate)]` attribute.
///
/// Returns `Ok(true)` if the annotated item should be kept, `Ok(false)` if it
/// should be removed.
pub fn eval_cfg_attribute(
    attr: &Attribute,
    cfg_options: &CfgOptions,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<bool, ExpandError> {
    let predicate = cfg_predicate_from_args(&attr.args, interner, ctx, attr.span)?;
    Ok(cfg_options.check(&predicate))
}

/// Evaluate a `#[cfg_attr(predicate, attrs...)]` attribute.
///
/// Returns the attributes that should be applied to the item when the predicate
/// is true. When false, returns an empty vector.
pub fn eval_cfg_attr_attribute(
    attr: &Attribute,
    cfg_options: &CfgOptions,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Vec<Attribute>, ExpandError> {
    let (predicate_expr, attr_exprs) = cfg_attr_split_args(&attr.args, attr.span)?;
    let predicate = cfg_predicate_from_expr(predicate_expr, interner, ctx, attr.span)?;
    if !cfg_options.check(&predicate) {
        return Ok(Vec::new());
    }

    let mut attrs = Vec::new();
    for expr in attr_exprs {
        attrs.extend(attributes_from_cfg_attr_expr(expr, attr.span)?);
    }
    Ok(attrs)
}

fn cfg_predicate_from_args(
    args: &AttributeArgs,
    interner: &Interner,
    ctx: &EagerContext<'_>,
    span: yelang_lexer::Span,
) -> Result<CfgPredicate, ExpandError> {
    let expr = single_cfg_predicate_expr(args, span)?;
    cfg_predicate_from_expr(expr, interner, ctx, span)
}

fn cfg_predicate_from_expr(
    expr: &Expr,
    interner: &Interner,
    ctx: &EagerContext<'_>,
    span: yelang_lexer::Span,
) -> Result<CfgPredicate, ExpandError> {
    let tokens = expr_to_token_stream(expr, interner);
    parse_cfg_predicate(&tokens, ctx, span)
}

fn single_cfg_predicate_expr(
    args: &AttributeArgs,
    span: yelang_lexer::Span,
) -> Result<&Expr, ExpandError> {
    let exprs = match args {
        AttributeArgs::Positional(exprs) => exprs,
        _ => {
            return Err(ExpandError::malformed_macro_args(
                "cfg predicate must be a single predicate expression".to_string(),
                span,
            ));
        }
    };
    if exprs.len() != 1 {
        return Err(ExpandError::malformed_macro_args(
            "cfg predicate must be a single predicate expression".to_string(),
            span,
        ));
    }
    Ok(&exprs[0])
}

fn cfg_attr_split_args(
    args: &AttributeArgs,
    span: yelang_lexer::Span,
) -> Result<(&Expr, &[Expr]), ExpandError> {
    let exprs = match args {
        AttributeArgs::Positional(exprs) => exprs,
        _ => {
            return Err(ExpandError::malformed_macro_args(
                "cfg_attr requires a predicate followed by attributes".to_string(),
                span,
            ));
        }
    };
    if exprs.len() < 2 {
        return Err(ExpandError::malformed_macro_args(
            "cfg_attr requires a predicate and at least one conditional attribute".to_string(),
            span,
        ));
    }
    Ok((&exprs[0], &exprs[1..]))
}

fn expr_to_token_stream(expr: &Expr, interner: &Interner) -> TokenStream {
    let mut rendered = String::new();
    // Codegen is best-effort for cfg predicates; ignore errors.
    let _ = expr.codegen(&mut rendered, interner);
    tokenize_rendered(&rendered, interner)
}

fn tokenize_rendered(rendered: &str, interner: &Interner) -> TokenStream {
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(rendered, &local_interner).unwrap_or_else(|_| {
        yelang_lexer::TokenStream::new_with_tokens(Vec::new(), local_interner.clone())
    });
    let tokens: Vec<_> = std::iter::from_fn(|| lex.advance().cloned()).collect();
    yelang_ast::expr::convert::from_lexer_tokens(&tokens, interner)
}

fn attributes_from_cfg_attr_expr(
    expr: &Expr,
    span: yelang_lexer::Span,
) -> Result<Vec<Attribute>, ExpandError> {
    match &expr.kind {
        ExprKind::Path(path) if path.segments.len() == 1 => {
            let name = path.segments[0].ident;
            Ok(vec![Attribute {
                path: vec![name],
                is_absolute: false,
                args: AttributeArgs::Empty,
                span: expr.span,
            }])
        }
        ExprKind::Object(obj) => {
            let mut attrs = Vec::new();
            for field in obj.fields() {
                let value = field.value();
                let args = if is_empty_object(value) {
                    AttributeArgs::Empty
                } else {
                    AttributeArgs::Positional(vec![value.clone()])
                };
                attrs.push(Attribute {
                    path: vec![*field.key()],
                    is_absolute: false,
                    args,
                    span: value.span,
                });
            }
            Ok(attrs)
        }
        _ => Err(ExpandError::malformed_macro_args(
            "cfg_attr conditional attribute must be a path or nested meta item".to_string(),
            span,
        )),
    }
}

fn is_empty_object(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Object(obj) if obj.fields().is_empty())
}
