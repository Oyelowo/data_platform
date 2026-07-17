//! AST code generation for the built-in `quote!` and `quote_spanned!` macros.
//!
//! The expansion produces a Yelang block expression that, when evaluated,
//! constructs and returns a `proc_macro::TokenStream`.

use yelang_ast::{
    BlockExpr, CallArgument, CallExpr, Expr, ExprKind, ForLoopExpr, Ident as AstIdent, LetStmt,
    Literal as AstLiteral, MethodCallExpr, Path, PathSegment, Pattern, PatternKind, Stmt, StmtKind,
    StrKind, StringLit,
};
use yelang_interner::Interner;
use yelang_lexer::Span;
use yelang_macro_core::token_tree::{
    Delimiter, Group as CoreGroup, Ident as CoreIdent, Literal as CoreLiteral, Punct as CorePunct,
    Spacing, TokenStream as CoreTokenStream, TokenTree,
};

use super::parse::{Node, RepKind, Template};

/// Name of the generated mutable stream variable.
const STREAM_VAR: &str = "__yelang_quote_stream";
/// Name of the generated span variable.
const SPAN_VAR: &str = "__yelang_quote_span";

/// Expand a `quote!` template into a Yelang expression.
///
/// `span_expr` is `None` for `quote!` (uses `proc_macro::Span::call_site()`)
/// and `Some(...)` for `quote_spanned!`.
pub fn expand_quote(
    template: &Template,
    span_expr: Option<&CoreTokenStream>,
    interner: &Interner,
    span: Span,
) -> Expr {
    let mut stmts = Vec::new();

    // `let __yelang_quote_span = <span>;`
    let span_init = match span_expr {
        Some(expr_tokens) => token_stream_to_expr(expr_tokens, interner, span),
        None => call(
            path_expr(&["proc_macro", "Span", "call_site"], span, interner),
            vec![],
            span,
        ),
    };
    stmts.push(let_stmt(SPAN_VAR, span_init, true, span, interner));

    // `let mut __yelang_quote_stream = proc_macro::TokenStream::new();`
    let stream_init = call(
        path_expr(&["proc_macro", "TokenStream", "new"], span, interner),
        vec![],
        span,
    );
    stmts.push(let_mut_stmt(STREAM_VAR, stream_init, span, interner));

    // Push each template node.
    for node in &template.nodes {
        push_node_stmts(&mut stmts, node, interner, span);
    }

    // Trailing expression: `__yelang_quote_stream`
    stmts.push(Stmt {
        kind: StmtKind::Expr(Box::new(path_expr(&[STREAM_VAR], span, interner))),
        span,
    });

    Expr {
        kind: ExprKind::Block(BlockExpr {
            label: None,
            statements: stmts,
        }),
        span,
    }
}

fn push_node_stmts(stmts: &mut Vec<Stmt>, node: &Node, interner: &Interner, span: Span) {
    match node {
        Node::Literal(tt) => push_literal_token(stmts, tt, interner, span),
        Node::Group { delimiter, nodes } => {
            let inner = nodes_to_stream_expr(nodes, interner, span);
            let group = call(
                path_expr(&["proc_macro", "Group", "new"], span, interner),
                vec![
                    delimiter_expr(*delimiter, span, interner),
                    inner,
                    path_expr(&[SPAN_VAR], span, interner),
                ],
                span,
            );
            let token_tree = call(
                path_expr(&["proc_macro", "TokenTree", "Group"], span, interner),
                vec![group],
                span,
            );
            stmts.push(stream_push_stmt(token_tree, span, interner));
        }
        Node::Interpolate { expr } => {
            let value = token_stream_to_expr(expr, interner, span);
            stmts.push(stream_extend_tokens_stmt(value, span, interner));
        }
        Node::Repetition {
            body,
            separator,
            kind,
        } => push_repetition_stmts(stmts, body, separator.as_ref(), *kind, interner, span),
    }
}

fn push_literal_token(stmts: &mut Vec<Stmt>, tt: &TokenTree, interner: &Interner, span: Span) {
    let token_tree_expr = match tt {
        TokenTree::Ident(i) => {
            let text = i.resolve(interner);
            let ident_new = call(
                path_expr(&["proc_macro", "Ident", "new"], span, interner),
                vec![
                    string_expr(text, span, interner),
                    path_expr(&[SPAN_VAR], span, interner),
                ],
                span,
            );
            call(
                path_expr(&["proc_macro", "TokenTree", "Ident"], span, interner),
                vec![ident_new],
                span,
            )
        }
        TokenTree::Punct(p) => {
            let punct_new = call(
                path_expr(&["proc_macro", "Punct", "new"], span, interner),
                vec![
                    char_expr(p.ch, span, interner),
                    spacing_expr(p.spacing, span, interner),
                    path_expr(&[SPAN_VAR], span, interner),
                ],
                span,
            );
            call(
                path_expr(&["proc_macro", "TokenTree", "Punct"], span, interner),
                vec![punct_new],
                span,
            )
        }
        TokenTree::Literal(l) => {
            let lit_new = literal_to_expr(l, interner, span);
            call(
                path_expr(&["proc_macro", "TokenTree", "Literal"], span, interner),
                vec![lit_new],
                span,
            )
        }
        TokenTree::Group(g) => {
            let inner =
                nodes_to_stream_expr(&parse_nodes(&g.stream, interner, span), interner, span);
            let group_new = call(
                path_expr(&["proc_macro", "Group", "new"], span, interner),
                vec![
                    delimiter_expr(g.delimiter, span, interner),
                    inner,
                    path_expr(&[SPAN_VAR], span, interner),
                ],
                span,
            );
            call(
                path_expr(&["proc_macro", "TokenTree", "Group"], span, interner),
                vec![group_new],
                span,
            )
        }
    };
    stmts.push(stream_push_stmt(token_tree_expr, span, interner));
}

/// Parse the nodes of a literal group so we can recursively expand them.
fn parse_nodes(stream: &CoreTokenStream, interner: &Interner, span: Span) -> Vec<Node> {
    use super::parse::parse;
    match parse(stream.clone()) {
        Ok(template) => template.nodes,
        Err(e) => {
            // Groups inside `quote!` should always parse successfully because
            // they are balanced delimiters. If they don't, generate a panic.
            vec![Node::Interpolate {
                expr: panic_expr(&e, interner, span),
            }]
        }
    }
}

fn nodes_to_stream_expr(nodes: &[Node], interner: &Interner, span: Span) -> Expr {
    let mut stmts = Vec::new();
    let inner_stream_var = "__yelang_quote_inner";
    let stream_init = call(
        path_expr(&["proc_macro", "TokenStream", "new"], span, interner),
        vec![],
        span,
    );
    stmts.push(let_mut_stmt(inner_stream_var, stream_init, span, interner));
    for node in nodes {
        push_node_stmts(&mut stmts, node, interner, span);
    }
    stmts.push(Stmt {
        kind: StmtKind::Expr(Box::new(path_expr(&[inner_stream_var], span, interner))),
        span,
    });
    Expr {
        kind: ExprKind::Block(BlockExpr {
            label: None,
            statements: stmts,
        }),
        span,
    }
}

fn push_repetition_stmts(
    stmts: &mut Vec<Stmt>,
    body: &[Node],
    separator: Option<&CorePunct>,
    kind: RepKind,
    interner: &Interner,
    span: Span,
) {
    // Collect the distinct interpolation expressions in the body.
    let interpolations = collect_interpolations(body);
    if interpolations.is_empty() {
        // Should have been caught by the parser, but guard anyway.
        stmts.push(stream_extend_tokens_stmt(
            call(
                path_expr(&["panic"], span, interner),
                vec![string_expr(
                    "quote! repetition has no interpolations",
                    span,
                    interner,
                )],
                span,
            ),
            span,
            interner,
        ));
        return;
    }

    let (pat, iter_expr) = if interpolations.len() == 1 {
        let elem_var = "__yelang_quote_elem";
        (
            binding_pattern(elem_var, span, interner),
            token_stream_to_expr(&interpolations[0], interner, span),
        )
    } else {
        // Zip multiple iterables together. We call `.iter()` on each and zip.
        let elem_vars: Vec<String> = (0..interpolations.len())
            .map(|i| format!("__yelang_quote_elem_{i}"))
            .collect();
        let pat = tuple_pattern(
            elem_vars.iter().map(|v| v.as_str()).collect(),
            span,
            interner,
        );
        let first = method_call(
            token_stream_to_expr(&interpolations[0], interner, span),
            "iter",
            vec![],
            span,
            interner,
        );
        let iter_expr = interpolations[1..].iter().fold(first, |acc, expr| {
            method_call(
                acc,
                "zip",
                vec![method_call(
                    token_stream_to_expr(expr, interner, span),
                    "iter",
                    vec![],
                    span,
                    interner,
                )],
                span,
                interner,
            )
        });
        (pat, iter_expr)
    };

    let mut body_stmts = Vec::new();

    // For `+`, the iterator must be non-empty; we rely on the source iterable.
    let _ = kind;

    // Separator before subsequent elements.
    if let Some(sep) = separator {
        let sep_push = stream_push_stmt(
            call(
                path_expr(&["proc_macro", "TokenTree", "Punct"], span, interner),
                vec![call(
                    path_expr(&["proc_macro", "Punct", "new"], span, interner),
                    vec![
                        char_expr(sep.ch, span, interner),
                        spacing_expr(sep.spacing, span, interner),
                        path_expr(&[SPAN_VAR], span, interner),
                    ],
                    span,
                )],
                span,
            ),
            span,
            interner,
        );
        body_stmts.push(conditional_separator_stmt(sep_push, span, interner));
    }

    // Remap interpolations in the body to the element variables.
    let mut remap_index = 0;
    let remapped_body = remap_interpolations(
        body,
        &interpolations,
        &elem_vars_for_count(interpolations.len()),
        &mut remap_index,
    );
    for node in &remapped_body {
        push_node_stmts(&mut body_stmts, node, interner, span);
    }

    stmts.push(Stmt {
        kind: StmtKind::Expr(Box::new(Expr {
            kind: ExprKind::ForLoop(ForLoopExpr {
                label: None,
                pat,
                iter: Box::new(iter_expr),
                body: BlockExpr {
                    label: None,
                    statements: body_stmts,
                },
            }),
            span,
        })),
        span,
    });
}

fn elem_vars_for_count(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("__yelang_quote_elem_{i}"))
        .collect()
}

/// Replace each interpolation node in `body` with a reference to the
/// corresponding loop variable.
fn remap_interpolations(
    nodes: &[Node],
    originals: &[CoreTokenStream],
    replacements: &[String],
    index: &mut usize,
) -> Vec<Node> {
    nodes
        .iter()
        .map(|node| match node {
            Node::Interpolate { expr } => {
                let pos = originals.iter().position(|o| o == expr).unwrap_or_else(|| {
                    let idx = *index;
                    *index += 1;
                    idx
                });
                Node::Interpolate {
                    expr: replacement_stream(&replacements[pos]),
                }
            }
            Node::Group { delimiter, nodes } => Node::Group {
                delimiter: *delimiter,
                nodes: remap_interpolations(nodes, originals, replacements, index),
            },
            Node::Repetition {
                body,
                separator,
                kind,
            } => Node::Repetition {
                body: remap_interpolations(body, originals, replacements, index),
                separator: *separator,
                kind: *kind,
            },
            other => other.clone(),
        })
        .collect()
}

fn replacement_stream(name: &str) -> CoreTokenStream {
    CoreTokenStream::from_vec(vec![TokenTree::Ident(CoreIdent::new(
        yelang_interner::Interner::new().get_or_intern(name),
        yelang_macro_core::token_tree::Span::default(),
    ))])
}

fn collect_interpolations(nodes: &[Node]) -> Vec<CoreTokenStream> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Interpolate { expr } => {
                if !out.contains(expr) {
                    out.push(expr.clone());
                }
            }
            Node::Group { nodes, .. } => out.extend(collect_interpolations(nodes)),
            Node::Repetition { body, .. } => out.extend(collect_interpolations(body)),
            Node::Literal(_) => {}
        }
    }
    out
}

fn conditional_separator_stmt(sep_push: Stmt, span: Span, interner: &Interner) -> Stmt {
    // `if !__yelang_quote_stream.is_empty() { <sep_push>; }`
    let cond = Expr {
        kind: ExprKind::Unary(yelang_ast::UnaryExpr {
            op: yelang_ast::UnaryOp::Not,
            expr: Box::new(method_call(
                path_expr(&[STREAM_VAR], span, interner),
                "is_empty",
                vec![],
                span,
                interner,
            )),
        }),
        span,
    };
    Stmt {
        kind: StmtKind::Expr(Box::new(Expr {
            kind: ExprKind::If(yelang_ast::IfExpr {
                condition: Box::new(cond),
                then_block: BlockExpr {
                    label: None,
                    statements: vec![sep_push],
                },
                else_expr: None,
            }),
            span,
        })),
        span,
    }
}

// --- AST construction helpers ------------------------------------------------

fn path_expr(segments: &[&str], span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Path(Path {
            qself: None,
            segments: segments
                .iter()
                .map(|s| PathSegment {
                    ident: AstIdent::new(interner.get_or_intern(s), span),
                    args: None,
                })
                .collect(),
            is_absolute: false,
            span,
        }),
        span,
    }
}

fn call(callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Call(CallExpr {
            callee: Box::new(callee),
            args: args.into_iter().map(CallArgument::Positional).collect(),
        }),
        span,
    }
}

fn method_call(
    receiver: Expr,
    method: &str,
    args: Vec<Expr>,
    span: Span,
    interner: &Interner,
) -> Expr {
    Expr {
        kind: ExprKind::MethodCall(MethodCallExpr {
            receiver: Box::new(receiver),
            segment: PathSegment {
                ident: AstIdent::new(interner.get_or_intern(method), span),
                args: None,
            },
            arguments: args.into_iter().map(CallArgument::Positional).collect(),
        }),
        span,
    }
}

fn string_expr(value: &str, span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Literal(AstLiteral::Str(StringLit {
            value: interner.get_or_intern(value),
            kind: StrKind::Normal,
        })),
        span,
    }
}

fn char_expr(ch: char, span: Span, interner: &Interner) -> Expr {
    // Yelang char literal is represented as a string literal of length 1 for
    // simplicity in the AST.
    let mut s = String::new();
    s.push(ch);
    string_expr(&s, span, interner)
}

fn delimiter_expr(delimiter: Delimiter, span: Span, interner: &Interner) -> Expr {
    let variant = match delimiter {
        Delimiter::Parenthesis => "Parenthesis",
        Delimiter::Brace => "Brace",
        Delimiter::Bracket => "Bracket",
        Delimiter::None => "None",
    };
    path_expr(&["proc_macro", "Delimiter", variant], span, interner)
}

fn spacing_expr(spacing: Spacing, span: Span, interner: &Interner) -> Expr {
    let variant = match spacing {
        Spacing::Alone => "Alone",
        Spacing::Joint => "Joint",
    };
    path_expr(&["proc_macro", "Spacing", variant], span, interner)
}

fn literal_to_expr(lit: &CoreLiteral, interner: &Interner, span: Span) -> Expr {
    // Render the core literal to source text and emit a call to
    // `proc_macro::Literal::from_source_text` so that the constructed literal
    // carries the correct span.
    let source = yelang_macro_core::token_tree::render::render_literal(lit, interner);
    call(
        path_expr(
            &["proc_macro", "Literal", "from_source_text"],
            span,
            interner,
        ),
        vec![
            string_expr(&source, span, interner),
            path_expr(&[SPAN_VAR], span, interner),
        ],
        span,
    )
}

fn token_stream_to_expr(tokens: &CoreTokenStream, interner: &Interner, span: Span) -> Expr {
    // Best-effort: render the tokens and re-parse them as a Yelang expression.
    let rendered = tokens.render(interner);
    let local_interner = interner.clone();
    let mut lex = match yelang_ast::TokenKind::tokenize(&rendered, &local_interner) {
        Ok(l) => l,
        Err(_) => {
            return call(
                path_expr(&["panic"], span, interner),
                vec![string_expr(
                    "invalid quote interpolation expression",
                    span,
                    interner,
                )],
                span,
            );
        }
    };
    match lex.parse::<Expr>() {
        Ok(expr) => expr,
        Err(_) => call(
            path_expr(&["panic"], span, interner),
            vec![string_expr(
                "invalid quote interpolation expression",
                span,
                interner,
            )],
            span,
        ),
    }
}

fn let_stmt(name: &str, init: Expr, _mutable: bool, span: Span, interner: &Interner) -> Stmt {
    Stmt {
        kind: StmtKind::Let(Box::new(LetStmt {
            pattern: Box::new(Pattern {
                pattern: PatternKind::Binding {
                    name: AstIdent::new(interner.get_or_intern(name), span),
                    mutability: yelang_ast::Mutability::Immutable,
                    subpattern: None,
                },
                span,
            }),
            ty: None,
            init: Some(Box::new(init)),
            span,
            attrs: vec![],
        })),
        span,
    }
}

fn let_mut_stmt(name: &str, init: Expr, span: Span, interner: &Interner) -> Stmt {
    Stmt {
        kind: StmtKind::Let(Box::new(LetStmt {
            pattern: Box::new(Pattern {
                pattern: PatternKind::Binding {
                    name: AstIdent::new(interner.get_or_intern(name), span),
                    mutability: yelang_ast::Mutability::Mutable,
                    subpattern: None,
                },
                span,
            }),
            ty: None,
            init: Some(Box::new(init)),
            span,
            attrs: vec![],
        })),
        span,
    }
}

fn binding_pattern(name: &str, span: Span, interner: &Interner) -> Pattern {
    Pattern {
        pattern: PatternKind::Binding {
            name: AstIdent::new(interner.get_or_intern(name), span),
            mutability: yelang_ast::Mutability::Immutable,
            subpattern: None,
        },
        span,
    }
}

fn tuple_pattern(names: Vec<&str>, span: Span, interner: &Interner) -> Pattern {
    Pattern {
        pattern: PatternKind::Tuple {
            patterns: names
                .into_iter()
                .map(|n| binding_pattern(n, span, interner))
                .collect(),
        },
        span,
    }
}

fn stream_push_stmt(token_tree: Expr, span: Span, interner: &Interner) -> Stmt {
    Stmt {
        kind: StmtKind::TermExpr(Box::new(method_call(
            path_expr(&[STREAM_VAR], span, interner),
            "push",
            vec![token_tree],
            span,
            interner,
        ))),
        span,
    }
}

fn stream_extend_tokens_stmt(value: Expr, span: Span, interner: &Interner) -> Stmt {
    Stmt {
        kind: StmtKind::TermExpr(Box::new(method_call(
            path_expr(&[STREAM_VAR], span, interner),
            "extend",
            vec![method_call(
                value,
                "to_token_stream",
                vec![],
                span,
                interner,
            )],
            span,
            interner,
        ))),
        span,
    }
}

fn panic_expr(message: &str, interner: &Interner, _span: Span) -> CoreTokenStream {
    let mut s = CoreTokenStream::new();
    s.push(TokenTree::Ident(CoreIdent::new(
        interner.get_or_intern("panic"),
        yelang_macro_core::token_tree::Span::default(),
    )));
    s.push(TokenTree::Punct(CorePunct::alone(
        '!',
        yelang_macro_core::token_tree::Span::default(),
    )));
    s.push(TokenTree::Group(CoreGroup::new(
        Delimiter::Parenthesis,
        CoreTokenStream::from_vec(vec![TokenTree::Literal(CoreLiteral::string(
            interner.get_or_intern(message),
            yelang_macro_core::token_tree::Span::default(),
        ))]),
        yelang_macro_core::token_tree::Span::default(),
    )));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_macros::expand_builtin_macro;
    use crate::quote_macro::parse;
    use yelang_macro_core::token_tree::{Ident as CoreIdent, Punct as CorePunct, Span as CoreSpan};

    fn interner() -> Interner {
        Interner::new()
    }

    fn render_expr(expr: &Expr, interner: &Interner) -> String {
        let mut buf = String::new();
        let _ = yelang_ast::Codegen::codegen(expr, &mut buf, interner);
        buf
    }

    fn tokenize_to_core(src: &str, interner: &Interner) -> CoreTokenStream {
        let local_interner = interner.clone();
        let mut lex = yelang_ast::TokenKind::tokenize(src, &local_interner).unwrap_or_default();
        let tokens: Vec<yelang_lexer::Token<yelang_ast::TokenKind>> =
            std::iter::from_fn(|| lex.advance().cloned()).collect();
        yelang_ast::expr::convert::from_lexer_tokens(&tokens, interner)
    }

    fn parse_template(src: &str, interner: &Interner) -> Template {
        let tokens = tokenize_to_core(src, interner);
        parse::parse(tokens).expect("template should parse")
    }

    fn template_literal_ident(name: &str, interner: &Interner) -> Template {
        Template {
            nodes: vec![Node::Literal(TokenTree::Ident(CoreIdent::new(
                interner.get_or_intern(name),
                CoreSpan::default(),
            )))],
        }
    }

    fn template_interpolation(name: &str, interner: &Interner) -> Template {
        let mut expr = CoreTokenStream::new();
        expr.push(TokenTree::Ident(CoreIdent::new(
            interner.get_or_intern(name),
            CoreSpan::default(),
        )));
        Template {
            nodes: vec![Node::Interpolate { expr }],
        }
    }

    #[test]
    fn expand_empty_quote_renders_stream_new() {
        let interner = interner();
        let template = Template { nodes: vec![] };
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("__yelang_quote_stream"));
        assert!(rendered.contains("proc_macro::TokenStream::new"));
    }

    #[test]
    fn expand_literal_ident_emits_push() {
        let interner = interner();
        let template = template_literal_ident("fn", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("proc_macro::Ident::new"));
        assert!(rendered.contains("\"fn\""));
        assert!(rendered.contains("push"));
    }

    #[test]
    fn expand_interpolation_emits_extend() {
        let interner = interner();
        let template = template_interpolation("name", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("extend"));
        assert!(rendered.contains("to_token_stream"));
        assert!(rendered.contains("name"));
    }

    #[test]
    fn expand_quote_spanned_uses_provided_span() {
        let interner = interner();
        let mut span_expr = CoreTokenStream::new();
        span_expr.push(TokenTree::Ident(CoreIdent::new(
            interner.get_or_intern("my_span"),
            CoreSpan::default(),
        )));
        let template = template_literal_ident("x", &interner);
        let expr = expand_quote(&template, Some(&span_expr), &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("my_span"));
    }

    #[test]
    fn expand_punctuation_emits_punct_new() {
        let interner = interner();
        let template = Template {
            nodes: vec![Node::Literal(TokenTree::Punct(CorePunct::alone(
                ':',
                CoreSpan::default(),
            )))],
        };
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("proc_macro::Punct::new"));
        assert!(rendered.contains("\":\""));
    }

    #[test]
    fn expand_group_emits_group_new() {
        let interner = interner();
        let template = parse_template("{ fn foo() {} }", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("proc_macro::Group::new"));
        assert!(rendered.contains("Delimiter::Brace"));
    }

    #[test]
    fn expand_repetition_star_emits_for_loop() {
        let interner = interner();
        let template = parse_template("#(#field)*", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("for"));
        assert!(rendered.contains("__yelang_quote_elem"));
        assert!(rendered.contains("extend"));
    }

    #[test]
    fn expand_repetition_with_separator_emits_conditional_push() {
        let interner = interner();
        let template = parse_template("#(#field),*", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("is_empty"));
        assert!(rendered.contains("proc_macro::Punct::new"));
    }

    #[test]
    fn expand_double_hash_emits_literal_hash_token() {
        let interner = interner();
        let template = parse_template("##flag", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("proc_macro::Punct::new"));
        assert!(rendered.contains("\"#\""));
    }

    #[test]
    fn expand_literal_string_uses_from_source_text() {
        let interner = interner();
        let template = parse_template("\"hello world\"", &interner);
        let expr = expand_quote(&template, None, &interner, Span::default());
        let rendered = render_expr(&expr, &interner);
        assert!(rendered.contains("from_source_text"));
        assert!(rendered.contains("\"hello world\""));
    }

    #[test]
    fn expand_builtin_quote_invocation_path() {
        let interner = interner();
        let src = "quote!(fn #name() {})";
        let mut lex = yelang_ast::TokenKind::tokenize(src, &interner).unwrap();
        let expr = lex.parse::<yelang_ast::Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected macro invocation");
        };
        let expanded = expand_builtin_macro(&inv, &interner).expect("quote! should expand");
        let rendered = render_expr(&expanded, &interner);
        assert!(rendered.contains("__yelang_quote_stream"));
        assert!(rendered.contains("proc_macro::Ident::new"));
    }
}
