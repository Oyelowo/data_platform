use yelang_ast::{
    BinaryExpr, BlockExpr, Expr, ExprKind, IfExpr, Literal, MacroArgs, MacroInvocation,
    MemberAccess, Path, PathSegment, StrKind, StringLit, UnaryExpr,
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
        BuiltinMacro::Panic => Some(expand_panic(inv, interner)),
        BuiltinMacro::Todo => Some(expand_todo(inv, interner)),
        BuiltinMacro::Unreachable => Some(expand_unreachable(inv, interner)),
        _ => None, // assert_eq, assert_ne, format: not yet implemented
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
    fn builtin_macro_from_path() {
        let mut interner = Interner::new();
        let path = simple_path("assert", Span::default(), &interner);
        assert_eq!(BuiltinMacro::from_path(&path, &interner), Some(BuiltinMacro::Assert));
    }
}
