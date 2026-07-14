use yelang_ast::{
    BinaryExpr, BlockExpr, Expr, ExprKind, IfExpr, Literal, MacroArgs, MacroInvocation,
    MemberAccess, Path, PathSegment, Stmt, StmtKind, StrKind, StringLit, UnaryExpr,
};
use yelang_interner::Interner;
use yelang_lexer::Span;

/// Built-in macros recognized by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinMacro {
    Assert,
    AssertEq,
    AssertNe,
    Panic,
    Todo,
    Unreachable,
    Format,
}

impl BuiltinMacro {
    /// Try to parse a macro path into a known built-in macro.
    pub fn from_path(path: &Path, interner: &Interner) -> Option<Self> {
        if path.segments.len() != 1 {
            return None;
        }
        let name = interner.resolve(&path.segments[0].ident.symbol);
        match name {
            "assert" => Some(BuiltinMacro::Assert),
            "assert_eq" => Some(BuiltinMacro::AssertEq),
            "assert_ne" => Some(BuiltinMacro::AssertNe),
            "panic" => Some(BuiltinMacro::Panic),
            "todo" => Some(BuiltinMacro::Todo),
            "unreachable" => Some(BuiltinMacro::Unreachable),
            "format" => Some(BuiltinMacro::Format),
            _ => None,
        }
    }
}

/// Expand a built-in macro invocation into a regular AST expression.
///
/// Returns `None` if the macro is not a recognized built-in.
pub fn expand_builtin_macro(
    inv: &MacroInvocation,
    interner: &Interner,
) -> Option<Expr> {
    let builtin = BuiltinMacro::from_path(&inv.path, interner)?;
    match builtin {
        BuiltinMacro::Assert => Some(expand_assert(inv, interner)),
        BuiltinMacro::AssertEq => Some(expand_assert_eq(inv, interner)),
        BuiltinMacro::AssertNe => Some(expand_assert_ne(inv, interner)),
        BuiltinMacro::Panic => Some(expand_panic(inv, interner)),
        BuiltinMacro::Todo => Some(expand_todo(inv, interner)),
        BuiltinMacro::Unreachable => Some(expand_unreachable(inv, interner)),
        BuiltinMacro::Format => Some(expand_format(inv, interner)),
    }
}

/// `assert!(cond)` → `if !cond { panic!("assertion failed") }`
/// `assert!(cond, msg)` → `if !cond { panic!(msg) }`
pub fn expand_assert(inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = inv.span;
    let args = match &inv.args {
        MacroArgs::Paren(args) => args.clone(),
        _ => return panic_expr("assert! requires parenthesized arguments", span, interner),
    };

    if args.is_empty() {
        return panic_expr("assert! requires at least one argument", span, interner);
    }

    let cond = args[0].clone();
    let msg = if args.len() >= 2 {
        args[1].clone()
    } else {
        string_literal("assertion failed", span, interner)
    };

    // Build: `if !cond { panic!(msg) }`
    let negated_cond = Expr {
        kind: ExprKind::Unary(UnaryExpr {
            op: yelang_ast::UnaryOp::Bang,
            expr: Box::new(cond),
        }),
        span,
    };

    let panic_call = Expr {
        kind: ExprKind::MacroInvocation(MacroInvocation {
            path: simple_path("panic", span, interner),
            args: MacroArgs::Paren(vec![msg]),
            span,
        }),
        span,
    };

    Expr {
        kind: ExprKind::If(IfExpr {
            condition: Box::new(negated_cond),
            then_block: BlockExpr {
                label: None,
                statements: vec![yelang_ast::Stmt {
                    kind: yelang_ast::StmtKind::Expr(Box::new(panic_call)),
                    span,
                }],
            },
            else_expr: None,
        }),
        span,
    }
}

/// `panic!(msg)` → a macro invocation that will later be lowered to a panic call.
/// For now, we expand it to a call to `std::panic`.
pub fn expand_panic(inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = inv.span;
    let args = match &inv.args {
        MacroArgs::Paren(args) => args.clone(),
        _ => return panic_expr("panic! requires parenthesized arguments", span, interner),
    };

    let msg = args.first().cloned().unwrap_or_else(|| string_literal("panic", span, interner));

    // Expand to a call expression: `panic(msg)` where `panic` is a function.
    Expr {
        kind: ExprKind::Call(yelang_ast::CallExpr {
            callee: Box::new(Expr {
                kind: ExprKind::Path(simple_path("panic", span, interner)),
                span,
            }),
            args: vec![yelang_ast::CallArgument::Positional(msg)],
        }),
        span,
    }
}

/// `todo!()` → `panic!("not yet implemented")`
pub fn expand_todo(_inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = _inv.span;
    Expr {
        kind: ExprKind::MacroInvocation(MacroInvocation {
            path: simple_path("panic", span, interner),
            args: MacroArgs::Paren(vec![string_literal("not yet implemented", span, interner)]),
            span,
        }),
        span,
    }
}

/// `unreachable!()` → `panic!("unreachable code")`
pub fn expand_unreachable(_inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = _inv.span;
    Expr {
        kind: ExprKind::MacroInvocation(MacroInvocation {
            path: simple_path("panic", span, interner),
            args: MacroArgs::Paren(vec![string_literal("unreachable code", span, interner)]),
            span,
        }),
        span,
    }
}

/// `assert_eq!(left, right)` →
/// ```ignore
/// {
///     let left_val = left;
///     let right_val = right;
///     if left_val != right_val {
///         panic!("assertion failed: `(left == right)`");
///     }
/// }
/// ```
pub fn expand_assert_eq(inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = inv.span;
    let args = match &inv.args {
        MacroArgs::Paren(args) => args.clone(),
        _ => return panic_expr("assert_eq! requires parenthesized arguments", span, interner),
    };
    if args.len() < 2 {
        return panic_expr("assert_eq! requires two arguments", span, interner);
    }

    let left = args[0].clone();
    let right = args[1].clone();
    build_assert_eq_ne(left, right, span, interner, /* is_eq = */ true)
}

/// `assert_ne!(left, right)` → similar to assert_eq! but with `==` and opposite condition.
pub fn expand_assert_ne(inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = inv.span;
    let args = match &inv.args {
        MacroArgs::Paren(args) => args.clone(),
        _ => return panic_expr("assert_ne! requires parenthesized arguments", span, interner),
    };
    if args.len() < 2 {
        return panic_expr("assert_ne! requires two arguments", span, interner);
    }

    let left = args[0].clone();
    let right = args[1].clone();
    build_assert_eq_ne(left, right, span, interner, /* is_eq = */ false)
}

fn build_assert_eq_ne(left: Expr, right: Expr, span: Span, interner: &Interner, is_eq: bool) -> Expr {
    // Build: `left_val != right_val` for assert_eq, `left_val == right_val` for assert_ne
    let bin_op = if is_eq {
        yelang_ast::BinaryOp::Ne
    } else {
        yelang_ast::BinaryOp::Eq
    };
    let cond = Expr {
        kind: ExprKind::Binary(BinaryExpr {
            left: Box::new(path_expr("left_val", span, interner)),
            op: bin_op,
            right: Box::new(path_expr("right_val", span, interner)),
        }),
        span,
    };

    let panic_call = Expr {
        kind: ExprKind::MacroInvocation(MacroInvocation {
            path: simple_path("panic", span, interner),
            args: MacroArgs::Paren(vec![string_literal(
                if is_eq { "assertion failed: `(left == right)`" } else { "assertion failed: `(left != right)`" },
                span,
                interner,
            )]),
            span,
        }),
        span,
    };

    let if_stmt = Stmt {
        kind: StmtKind::Expr(Box::new(Expr {
            kind: ExprKind::If(IfExpr {
                condition: Box::new(cond),
                then_block: BlockExpr {
                    label: None,
                    statements: vec![Stmt {
                        kind: StmtKind::Expr(Box::new(panic_call)),
                        span,
                    }],
                },
                else_expr: None,
            }),
            span,
        })),
        span,
    };

    // Build the block: `{ let left_val = left; let right_val = right; if ... }`
    Expr {
        kind: ExprKind::Block(BlockExpr {
            label: None,
            statements: vec![
                let_stmt("left_val", left, span, interner),
                let_stmt("right_val", right, span, interner),
                if_stmt,
            ],
        }),
        span,
    }
}

fn let_stmt(name: &str, init: Expr, span: Span, interner: &Interner) -> Stmt {
    Stmt {
        kind: StmtKind::Let(Box::new(yelang_ast::LetStmt {
            pattern: Box::new(yelang_ast::Pattern {
                pattern: yelang_ast::PatternKind::Binding {
                    name: yelang_ast::Ident::new(interner.get_or_intern(name), span),
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

fn path_expr(name: &str, span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Path(simple_path(name, span, interner)),
        span,
    }
}

/// `format!("hello {name}", name = "world")` → a call to `format(...)`.
///
/// For now we expand to a runtime call `format(args...)` where the first arg
/// is the format string and the rest are the values.  The backend is expected
/// to provide a `format` function (or intrinsic) in the prelude.
pub fn expand_format(inv: &MacroInvocation, interner: &Interner) -> Expr {
    let span = inv.span;
    let args = match &inv.args {
        MacroArgs::Paren(args) => args.clone(),
        _ => return panic_expr("format! requires parenthesized arguments", span, interner),
    };
    if args.is_empty() {
        return panic_expr("format! requires at least one argument", span, interner);
    }

    let call_args: Vec<yelang_ast::CallArgument> = args
        .into_iter()
        .map(yelang_ast::CallArgument::Positional)
        .collect();

    Expr {
        kind: ExprKind::Call(yelang_ast::CallExpr {
            callee: Box::new(Expr {
                kind: ExprKind::Path(simple_path("format", span, interner)),
                span,
            }),
            args: call_args,
        }),
        span,
    }
}

// --- Helpers ---

fn simple_path(name: &str, span: Span, interner: &Interner) -> Path {
    Path {
        qself: None,
        segments: vec![PathSegment {
            ident: yelang_ast::Ident::new(interner.get_or_intern(name), span),
            args: None,
        }],
        is_absolute: false,
        span,
    }
}

fn string_literal(text: &str, span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Str(StringLit {
            value: interner.get_or_intern(text),
            kind: StrKind::Normal,
        })),
        span,
    }
}

fn panic_expr(msg: &str, span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::MacroInvocation(MacroInvocation {
            path: simple_path("panic", span, interner),
            args: MacroArgs::Paren(vec![string_literal(msg, span, interner)]),
            span,
        }),
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::TokenKind;
    use yelang_interner::Interner;
    use yelang_lexer::ParseTokenStream;

    fn parse_macro_invocation(src: &str) -> (MacroInvocation, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let expr = stream.parse::<Expr>().unwrap();
        let ExprKind::MacroInvocation(inv) = expr.kind else {
            panic!("expected macro invocation, got {:?}", expr.kind);
        };
        (inv, interner)
    }

    #[test]
    fn expand_assert_single_arg() {
        let (inv, interner) = parse_macro_invocation("assert!(true)");
        let expanded = expand_assert(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::If(_)), "assert! should expand to if");
    }

    #[test]
    fn expand_assert_two_args() {
        let (inv, interner) = parse_macro_invocation("assert!(x > 0, \"x must be positive\")");
        let expanded = expand_assert(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::If(_)));
    }

    #[test]
    fn expand_panic_with_message() {
        let (inv, interner) = parse_macro_invocation("panic!(\"something went wrong\")");
        let expanded = expand_panic(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::Call(_)), "panic! should expand to call");
    }

    #[test]
    fn test_expand_todo() {
        let (inv, interner) = parse_macro_invocation("todo!()");
        let expanded = expand_todo(&inv, &interner);
        let ExprKind::MacroInvocation(inner) = expanded.kind else {
            panic!("todo! should expand to panic! macro invocation");
        };
        assert_eq!(interner.resolve(&inner.path.segments[0].ident.symbol), "panic");
    }

    #[test]
    fn test_expand_unreachable() {
        let (inv, interner) = parse_macro_invocation("unreachable!()");
        let expanded = expand_unreachable(&inv, &interner);
        let ExprKind::MacroInvocation(inner) = expanded.kind else {
            panic!("unreachable! should expand to panic! macro invocation");
        };
        assert_eq!(interner.resolve(&inner.path.segments[0].ident.symbol), "panic");
    }

    #[test]
    fn expand_assert_eq_basic() {
        let (inv, interner) = parse_macro_invocation("assert_eq!(a, b)");
        let expanded = expand_assert_eq(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::Block(_)), "assert_eq! should expand to block");
    }

    #[test]
    fn expand_assert_ne_basic() {
        let (inv, interner) = parse_macro_invocation("assert_ne!(a, b)");
        let expanded = expand_assert_ne(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::Block(_)), "assert_ne! should expand to block");
    }

    #[test]
    fn expand_format_basic() {
        let (inv, interner) = parse_macro_invocation("format!(\"hello {}\", name)");
        let expanded = expand_format(&inv, &interner);
        assert!(matches!(expanded.kind, ExprKind::Call(_)), "format! should expand to call");
    }

    #[test]
    fn builtin_macro_from_path() {
        let mut interner = Interner::new();
        let path = simple_path("assert", Span::default(), &interner);
        assert_eq!(BuiltinMacro::from_path(&path, &interner), Some(BuiltinMacro::Assert));
    }
}
