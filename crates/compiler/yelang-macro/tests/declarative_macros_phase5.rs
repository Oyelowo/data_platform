use std::path::Path;

use yelang_ast::{Codegen, ExprKind, ItemKind, StmtKind};
use yelang_interner::Interner;
use yelang_macro::{CfgOptions, ExpandError, MacroExpander, MemoryEnvProvider, MemoryFileLoader};

fn parse_program(src: &str) -> (yelang_ast::Program, Interner) {
    let mut interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    (program, interner)
}

fn expand_with<'a>(
    program: &'a yelang_ast::Program,
    interner: &'a Interner,
    files: &'a MemoryFileLoader,
    env: &'a MemoryEnvProvider,
    cfg: CfgOptions,
) -> (yelang_ast::Program, Vec<ExpandError>) {
    let mut expander = MacroExpander::new(interner)
        .with_file_loader(files)
        .with_env_provider(env)
        .with_cfg_options(cfg)
        .with_current_file(Path::new("/test/main.ye"));
    let result = expander.expand(program);
    (result.program, result.errors)
}

fn main_body(program: &yelang_ast::Program) -> &yelang_ast::BlockExpr {
    let item = &program.items[0];
    let ItemKind::Fn(func) = &item.kind else {
        panic!("expected fn main");
    };
    &func.body
}

fn let_init<'a>(stmt: &'a StmtKind) -> &'a yelang_ast::Expr {
    match stmt {
        StmtKind::Let(l) => l.init.as_deref().expect("let has init"),
        _ => panic!("expected let statement"),
    }
}

fn expr_stmt(expr: &yelang_ast::Expr, interner: &Interner) -> String {
    let mut buf = String::new();
    expr.codegen(&mut buf, interner).unwrap();
    buf
}

// -----------------------------------------------------------------------------
// concat!
// -----------------------------------------------------------------------------

#[test]
fn concat_strings_and_chars() {
    let src = r#"
        fn main() {
            let a = concat!("a", 'b', 1, true);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "ab1true");
}

#[test]
fn concat_with_negative_numbers() {
    let src = r#"
        fn main() {
            let a = concat!(-1, " ", -3);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "-1 -3");
}

#[test]
fn concat_rejects_identifiers() {
    let src = r#"
        fn main() {
            let a = concat!(foo);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (_, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(
        errors.iter().any(|e| matches!(e, ExpandError::MalformedMacroArgs { reason, .. } if reason.contains("concat!"))),
        "expected concat! error, got {:?}",
        errors
    );
}

// -----------------------------------------------------------------------------
// concat_bytes!
// -----------------------------------------------------------------------------

#[test]
fn concat_bytes_literals_and_arrays() {
    let src = r#"
        fn main() {
            let a = concat_bytes!("ab", [0, 1, 2]);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "ab\x00\x01\x02");
}

#[test]
fn concat_bytes_rejects_bad_integer() {
    let src = r#"
        fn main() {
            let a = concat_bytes!(256);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (_, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(
        errors.iter().any(|e| matches!(e, ExpandError::MalformedMacroArgs { reason, .. } if reason.contains("concat_bytes!"))),
        "expected concat_bytes! error, got {:?}",
        errors
    );
}

// -----------------------------------------------------------------------------
// stringify!
// -----------------------------------------------------------------------------

#[test]
fn stringify_produces_string_literal() {
    let src = r#"
        fn main() {
            let a = stringify!(1 + 2);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    // Token-tree renderer emits minimal spacing.
    assert_eq!(interner.resolve(&s.value), "1+2");
}

// -----------------------------------------------------------------------------
// include!
// -----------------------------------------------------------------------------

#[test]
fn include_inserts_items() {
    let mut files = MemoryFileLoader::new();
    files.insert("/test/helper.ye", "fn helper() -> i32 { 42 }");

    let src = r#"
        fn main() {
            let a = helper();
        }
        include!("/test/helper.ye")
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &files,
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    assert_eq!(program.items.len(), 2, "expected main + helper");
}

#[test]
fn include_errors_for_missing_file() {
    let src = r#"
        fn main() {}
        include!("missing.ye")
    "#;
    let (program, interner) = parse_program(src);
    let (_, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(
        errors.iter().any(|e| matches!(e, ExpandError::MalformedMacroArgs { reason, .. } if reason.contains("include!"))),
        "expected include! error, got {:?}",
        errors
    );
}

// -----------------------------------------------------------------------------
// include_str!
// -----------------------------------------------------------------------------

#[test]
fn include_str_reads_file() {
    let mut files = MemoryFileLoader::new();
    files.insert("/test/data.txt", "hello world");

    let src = r#"
        fn main() {
            let a = include_str!("/test/data.txt");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &files,
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "hello world");
}

// -----------------------------------------------------------------------------
// include_bytes!
// -----------------------------------------------------------------------------

#[test]
fn include_bytes_reads_file() {
    let mut files = MemoryFileLoader::new();
    files.insert("/test/data.bin", "AB");

    let src = r#"
        fn main() {
            let a = include_bytes!("/test/data.bin");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &files,
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array, got {:?}", init.kind);
    };
    let elements = arr.elements().expect("array has elements");
    assert_eq!(elements.len(), 2);
}

// -----------------------------------------------------------------------------
// env! / option_env!
// -----------------------------------------------------------------------------

#[test]
fn env_looks_up_variable() {
    let mut env = MemoryEnvProvider::new();
    env.insert("YELANG_TEST", "hello");

    let src = r#"
        fn main() {
            let a = env!("YELANG_TEST");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &env,
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "hello");
}

#[test]
fn env_errors_for_missing_variable() {
    let src = r#"
        fn main() {
            let a = env!("MISSING_VAR");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (_, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(
        errors.iter().any(|e| matches!(e, ExpandError::MalformedMacroArgs { reason, .. } if reason.contains("environment variable"))),
        "expected env! error, got {:?}",
        errors
    );
}

#[test]
fn option_env_some_when_present() {
    let mut env = MemoryEnvProvider::new();
    env.insert("YELANG_OPT", "value");

    let src = r#"
        fn main() {
            let a = option_env!("YELANG_OPT");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &env,
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let rendered = expr_stmt(init, &interner);
    assert!(rendered.contains("Some"), "expected Some, got {}", rendered);
    assert!(
        rendered.contains("value"),
        "expected value, got {}",
        rendered
    );
}

#[test]
fn option_env_none_when_missing() {
    let src = r#"
        fn main() {
            let a = option_env!("MISSING_OPT");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let rendered = expr_stmt(init, &interner);
    assert!(rendered.contains("None"), "expected None, got {}", rendered);
}

// -----------------------------------------------------------------------------
// compile_error!
// -----------------------------------------------------------------------------

#[test]
fn compile_error_emits_error() {
    let src = r#"
        fn main() {
            compile_error!("this is an error");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (_, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(
        errors.iter().any(|e| matches!(e, ExpandError::MalformedMacroArgs { reason, .. } if reason == "this is an error")),
        "expected compile_error!, got {:?}",
        errors
    );
}

// -----------------------------------------------------------------------------
// cfg!
// -----------------------------------------------------------------------------

#[test]
fn cfg_evaluates_name() {
    let cfg = CfgOptions::new().with_name("unix");
    let src = r#"
        fn main() {
            let a = cfg!(unix);
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        cfg,
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Bool(b)) = &init.kind else {
        panic!("expected bool literal, got {:?}", init.kind);
    };
    assert!(*b);
}

#[test]
fn cfg_evaluates_key_value() {
    let cfg = CfgOptions::new().with_key_value("feature", "foo");
    let src = r#"
        fn main() {
            let a = cfg!(feature = "foo");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        cfg,
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Bool(b)) = &init.kind else {
        panic!("expected bool literal, got {:?}", init.kind);
    };
    assert!(*b);
}

#[test]
fn cfg_evaluates_not() {
    let cfg = CfgOptions::new().with_name("unix");
    let src = r#"
        fn main() {
            let a = cfg!(not(windows));
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        cfg,
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Bool(b)) = &init.kind else {
        panic!("expected bool literal, got {:?}", init.kind);
    };
    assert!(*b);
}

#[test]
fn cfg_evaluates_all_any() {
    let cfg = CfgOptions::new()
        .with_name("unix")
        .with_key_value("feature", "foo");
    let src = r#"
        fn main() {
            let a = cfg!(all(unix, feature = "foo"));
            let b = cfg!(any(windows, feature = "foo"));
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        cfg,
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let body = main_body(&program);
    let a = let_init(&body.statements[0].kind);
    let b = let_init(&body.statements[1].kind);
    let ExprKind::Literal(yelang_ast::Literal::Bool(a_val)) = &a.kind else {
        panic!("expected bool literal, got {:?}", a.kind);
    };
    let ExprKind::Literal(yelang_ast::Literal::Bool(b_val)) = &b.kind else {
        panic!("expected bool literal, got {:?}", b.kind);
    };
    assert!(*a_val);
    assert!(*b_val);
}

// -----------------------------------------------------------------------------
// Nested / combined eager expansion
// -----------------------------------------------------------------------------

#[test]
fn nested_eager_expansion() {
    let src = r#"
        fn main() {
            let a = concat!(stringify!(1), "x");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "1x");
}

#[test]
fn eager_inside_declarative_macro() {
    let src = r#"
        macro greet {
            ($name:expr) => ( concat!("hello ", $name) );
        }
        fn main() {
            let a = greet!("world");
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &MemoryFileLoader::new(),
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Literal(yelang_ast::Literal::Str(s)) = &init.kind else {
        panic!("expected string literal, got {:?}", init.kind);
    };
    assert_eq!(interner.resolve(&s.value), "hello world");
}

// -----------------------------------------------------------------------------
// item-position eager macros
// -----------------------------------------------------------------------------

#[test]
fn include_in_item_position() {
    let mut files = MemoryFileLoader::new();
    files.insert("/test/consts.ye", "const ANSWER: i32 = 42;");

    let src = r#"
        include!("/test/consts.ye")
        fn main() {
            let a = ANSWER;
        }
    "#;
    let (program, interner) = parse_program(src);
    let (program, errors) = expand_with(
        &program,
        &interner,
        &files,
        &MemoryEnvProvider::new(),
        CfgOptions::new(),
    );
    assert!(errors.is_empty(), "errors: {:?}", errors);
    assert_eq!(program.items.len(), 2);
}
