use yelang_ast::{Codegen, ExprKind, ItemKind, StmtKind};
use yelang_interner::Interner;
use yelang_macro::expand_program;

fn parse_and_expand(
    src: &str,
) -> (
    yelang_ast::Program,
    Interner,
    Vec<yelang_macro::ExpandError>,
) {
    let mut interner = Interner::new();
    let mut stream = yelang_ast::TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    let result = expand_program(&program, &interner);
    (result.program, interner, result.errors)
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

fn has_macro_def_error(errors: &[yelang_macro::ExpandError], needle: &str) -> bool {
    errors.iter().any(|e| match e {
        yelang_macro::ExpandError::MacroDefError { reason, .. } => reason.contains(needle),
        _ => false,
    })
}

fn has_transcribe_error(errors: &[yelang_macro::ExpandError], needle: &str) -> bool {
    errors.iter().any(|e| match e {
        yelang_macro::ExpandError::MacroTranscribeError { reason, .. } => reason.contains(needle),
        _ => false,
    })
}

fn int_literal_value<'a>(expr: &'a yelang_ast::Expr, interner: &'a Interner) -> Option<&'a str> {
    match &expr.kind {
        ExprKind::Literal(yelang_ast::Literal::Int(i)) => Some(interner.resolve(&i.value)),
        _ => None,
    }
}

// ============================================================
// Follow-set validation
// ============================================================

#[test]
fn expr_followed_by_comma_is_legal() {
    let src = r#"
        macro m { ($x:expr, $y:expr) => ( $x + $y ); }
        fn main() { let a = m!(1, 2); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(matches!(
        let_init(&main_body(&program).statements[0].kind).kind,
        ExprKind::Binary(_)
    ));
}

#[test]
fn expr_followed_by_lbracket_is_illegal() {
    let src = r#"
        macro m { ($x:expr[$y:expr]) => ( $x ); }
        fn main() { let a = m!(1[2]); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_macro_def_error(&errors, "may not be followed by"),
        "expected follow-set violation, got {:?}",
        errors
    );
}

#[test]
fn pat_followed_by_pipe_is_illegal() {
    // `|` is part of the or-pattern nonterminal, so a `pat` fragment cannot be
    // followed by `|` in the matcher.
    let src = r#"
        macro m { ($p:pat | $q:pat) => ( 1 ); }
        fn main() { let a = m!(x | y); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_macro_def_error(&errors, "may not be followed by"),
        "expected follow-set violation, got {:?}",
        errors
    );
}

#[test]
fn pat_followed_by_comma_is_legal_definition() {
    // The definition is legal; invocation matching relies on comma separation.
    let src = r#"
        macro m { ($p:pat, $e:expr) => ( 1 ); }
        fn main() { let a = m!(x, 1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn ty_followed_by_colon_is_legal_definition() {
    // `:` is in the `ty` follow set, so the definition is accepted.
    // Invoking this rule with the current comma-only fragment capture would not
    // match; this test verifies the definition-time future-proofing check.
    let src = r#"
        macro m { ($t:ty: $e:expr) => ( 1 ); }
        fn main() {}
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn ty_followed_by_minus_is_illegal() {
    let src = r#"
        macro m { ($t:ty -> $e:expr) => ( $e ); }
        fn main() { let a = m!(i32 -> 1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_macro_def_error(&errors, "may not be followed by"),
        "expected follow-set violation, got {:?}",
        errors
    );
}

#[test]
fn tt_followed_by_anything_is_legal() {
    let src = r#"
        macro m { ($x:tt + $y:tt) => ( 1 ); }
        fn main() { let a = m!(a + b); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn separated_repetition_separator_in_follow_set() {
    let src = r#"
        macro m { ($($x:expr),*) => ( 1 ); }
        fn main() { let a = m!(1, 2); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn separated_repetition_separator_not_in_follow_set() {
    let src = r#"
        macro m { ($($x:expr).*) => ( 1 ); }
        fn main() { let a = m!(1.2); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_macro_def_error(&errors, "may not be followed by"),
        "expected follow-set violation for separator, got {:?}",
        errors
    );
}

#[test]
fn unseparated_star_tt_can_follow_itself() {
    let src = r#"
        macro m { ($($x:tt)*) => ( 1 ); }
        fn main() { let a = m!(a b c); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

#[test]
fn unseparated_star_expr_cannot_follow_itself() {
    let src = r#"
        macro m { ($($x:expr)*) => ( 1 ); }
        fn main() { let a = m!(1 2); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_macro_def_error(&errors, "may not be followed by"),
        "expected follow-set violation for unseparated expr, got {:?}",
        errors
    );
}

#[test]
fn complex_matcher_follow_set_nested_repetition_valid() {
    let src = r#"
        macro matrix {
            ($([$($x:expr),*]),*) => ( [$([$($x),*]),*] );
        }
        fn main() { let a = matrix!([1, 2], [3, 4]); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}

// ============================================================
// Trailing separators
// ============================================================

#[test]
fn trailing_comma_in_star() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(1, 2, 3,); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(matches!(
        let_init(&main_body(&program).statements[0].kind).kind,
        ExprKind::Array(_)
    ));
}

#[test]
fn trailing_comma_in_plus() {
    let src = r#"
        macro list { ($($x:expr),+) => ( [$($x),+] ); }
        fn main() { let a = list!(1, 2, 3,); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(matches!(
        let_init(&main_body(&program).statements[0].kind).kind,
        ExprKind::Array(_)
    ));
}

#[test]
fn trailing_semicolon_in_star() {
    let src = r#"
        macro stmts { ($($s:stmt);*) => ( { $($s);* } ); }
        fn main() { let a = stmts!(let x = 1; let y = 2;); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(matches!(
        let_init(&main_body(&program).statements[0].kind).kind,
        ExprKind::Block(_)
    ));
}

#[test]
fn no_trailing_separator_still_works() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(1, 2, 3); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(matches!(
        let_init(&main_body(&program).statements[0].kind).kind,
        ExprKind::Array(_)
    ));
}

#[test]
fn double_trailing_separator_rejected() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(1, 2,,); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroMatchError { .. })),
        "expected match error, got {:?}",
        errors
    );
}

#[test]
fn trailing_separator_with_empty_star_rejected() {
    let src = r#"
        macro list { ($($x:expr),*) => ( [$($x),*] ); }
        fn main() { let a = list!(,); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroMatchError { .. })),
        "expected match error for lone trailing separator, got {:?}",
        errors
    );
}

// ============================================================
// Metavariable expressions — count
// ============================================================

#[test]
fn count_total_repetitions() {
    let src = r#"
        macro len { ($($x:expr),*) => ( ${count(x)} ); }
        fn main() { let a = len!(1, 2, 3); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert_eq!(int_literal_value(init, &interner).as_deref(), Some("3"));
}

#[test]
fn count_total_nested_repetitions() {
    let src = r#"
        macro total {
            ($([$($x:expr),*]),*) => ( ${count(x)} );
        }
        fn main() { let a = total!([1, 2, 3], [4, 5]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert_eq!(int_literal_value(init, &interner).as_deref(), Some("5"));
}

#[test]
fn count_with_depth_outer() {
    // Depth 0 for count = outer-most repetition count.
    let src = r#"
        macro counts {
            ($([$($x:expr),*]),*) => ( ${count(x, 0)} );
        }
        fn main() { let a = counts!([1, 2], [3], [4, 5, 6]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert_eq!(int_literal_value(init, &interner).as_deref(), Some("3"));
}

#[test]
fn count_with_depth_inner_sum() {
    // Depth 1 for count = sum of counts at the next nesting level.
    let src = r#"
        macro counts {
            ($([$($x:expr),*]),*) => ( ${count(x, 1)} );
        }
        fn main() { let a = counts!([1, 2], [3], [4, 5, 6]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert_eq!(int_literal_value(init, &interner).as_deref(), Some("6"));
}

// ============================================================
// Metavariable expressions — index and length
// ============================================================

#[test]
fn index_in_inner_repetition() {
    // `${ignore(x)}` drives the repetition so `${index()}` has a context.
    let src = r#"
        macro idx { ($($x:expr),*) => ( [$( ${ignore(x)} ${index()} ),*] ); }
        fn main() { let a = idx!(a, b, c); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };
    let values: Vec<Option<&str>> = arr
        .elements()
        .unwrap()
        .iter()
        .map(|e| int_literal_value(e, &interner))
        .collect();
    assert_eq!(values, vec![Some("0"), Some("1"), Some("2")]);
}

#[test]
fn len_in_inner_repetition() {
    let src = r#"
        macro lengths { ($($x:expr),*) => ( [$( ${ignore(x)} ${len()} ),*] ); }
        fn main() { let a = lengths!(a, b, c); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };
    for elem in arr.elements().unwrap() {
        assert_eq!(int_literal_value(elem, &interner).as_deref(), Some("3"));
    }
}

#[test]
fn index_at_outer_depth() {
    // Nested transcriber repetitions: inner array uses ${index(1)} to read the
    // outer repetition index.
    let src = r#"
        macro row_idx {
            ($([$($x:expr),*]),*) => ( [$([$(${ignore(x)} ${index(1)}),*]),*] );
        }
        fn main() { let a = row_idx!([1, 2, 3], [4, 5, 6]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };
    let outer = arr.elements().unwrap();
    assert_eq!(outer.len(), 2);

    fn first_inner_element<'a>(expr: &'a yelang_ast::Expr, interner: &'a Interner) -> &'a str {
        let ExprKind::Array(arr) = &expr.kind else {
            panic!("expected inner array");
        };
        int_literal_value(&arr.elements().unwrap()[0], interner).unwrap()
    }

    assert_eq!(first_inner_element(&outer[0], &interner), "0");
    assert_eq!(first_inner_element(&outer[1], &interner), "1");
}

#[test]
fn len_at_outer_depth() {
    let src = r#"
        macro row_len {
            ($([$($x:expr),*]),*) => ( [$([$(${ignore(x)} ${len(1)}),*]),*] );
        }
        fn main() { let a = row_len!([1, 2, 3], [4, 5, 6]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };
    for outer in arr.elements().unwrap() {
        let ExprKind::Array(inner) = &outer.kind else {
            panic!("expected inner array");
        };
        for elem in inner.elements().unwrap() {
            assert_eq!(int_literal_value(elem, &interner).as_deref(), Some("2"));
        }
    }
}

#[test]
fn nested_mixed_index_len_count() {
    // Two rows, first with 3 cols, second with 2 cols.
    // Emit tuples of (outer_index, inner_index, inner_len, value) in nested arrays.
    let src = r#"
        macro mixed {
            ($([$($x:expr),*]),*) => (
                [$([$(( ${index(1)}, ${index()}, ${len()}, $x )),*]),*]
            );
        }
        fn main() { let a = mixed!([1, 2, 3], [4, 5]); }
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let mut rendered = String::new();
    init.codegen(&mut rendered, &interner).unwrap();
    eprintln!("init rendered: {}", rendered);
    eprintln!("init kind: {:?}", init.kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };

    fn tuple_field(expr: &yelang_ast::Expr, idx: usize, interner: &Interner) -> String {
        let ExprKind::Tuple(tup) = &expr.kind else {
            panic!("expected tuple");
        };
        int_literal_value(&tup[idx], interner)
            .expect("expected int literal")
            .to_string()
    }

    fn array_elements(expr: &yelang_ast::Expr) -> &[yelang_ast::Expr] {
        let ExprKind::Array(arr) = &expr.kind else {
            panic!("expected array");
        };
        arr.elements().unwrap()
    }

    let outer = arr.elements().unwrap();
    assert_eq!(outer.len(), 2);

    let row0 = array_elements(&outer[0]);
    assert_eq!(row0.len(), 3);
    // Element 0: row 0, col 0 -> (0, 0, 3)
    assert_eq!(tuple_field(&row0[0], 0, &interner), "0");
    assert_eq!(tuple_field(&row0[0], 1, &interner), "0");
    assert_eq!(tuple_field(&row0[0], 2, &interner), "3");

    let row1 = array_elements(&outer[1]);
    assert_eq!(row1.len(), 2);
    // Element 3 (row 1, col 1) -> (1, 1, 2)
    assert_eq!(tuple_field(&row1[1], 0, &interner), "1");
    assert_eq!(tuple_field(&row1[1], 1, &interner), "1");
    assert_eq!(tuple_field(&row1[1], 2, &interner), "2");
}

// ============================================================
// Metavariable expressions — ignore
// ============================================================

#[test]
fn ignore_expands_to_nothing() {
    let src = r#"
        macro repeat_count {
            ($($x:expr),*) => ( [$( ${ignore(x)} 1 ),*] );
        }
        fn main() { let a = repeat_count!(a, b, c); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    let ExprKind::Array(arr) = &init.kind else {
        panic!("expected array");
    };
    assert_eq!(arr.elements().unwrap().len(), 3);
    for elem in arr.elements().unwrap() {
        let ExprKind::Literal(yelang_ast::Literal::Int(_)) = &elem.kind else {
            panic!("expected literal 1, got {:?}", elem.kind);
        };
    }
}

// ============================================================
// Metavariable expressions — in attribute macros
// ============================================================

#[test]
fn metavar_expr_in_attribute_transcriber() {
    let src = r#"
        macro tagged {
            attr($($t:expr),*)($item:item) => {
                const TAG_COUNT: i32 = ${count(t)};
                $item
            };
        }
        @tagged(1, 2, 3)
        fn main() {}
    "#;
    let (program, interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let consts: Vec<_> = program
        .items
        .iter()
        .filter(|i| matches!(i.kind, ItemKind::Const(_)))
        .collect();
    assert_eq!(consts.len(), 1);
    let ItemKind::Const(c) = &consts[0].kind else {
        unreachable!()
    };
    assert_eq!(int_literal_value(&c.value, &interner).as_deref(), Some("3"));
}

// ============================================================
// Negative / regression tests
// ============================================================

#[test]
fn unknown_metavar_expr_errors() {
    let src = r#"
        macro bad { ($x:expr) => ( ${foo(x)} ); }
        fn main() { let a = bad!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, yelang_macro::ExpandError::MacroDefError { .. })),
        "expected macro def error for invalid metavar expr, got {:?}",
        errors
    );
}

#[test]
fn count_of_unbound_metavar_errors() {
    let src = r#"
        macro bad { ($x:expr) => ( ${count(y)} ); }
        fn main() { let a = bad!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_transcribe_error(&errors, "unknown metavariable"),
        "expected transcribe error for unbound metavar, got {:?}",
        errors
    );
}

#[test]
fn index_outside_repetition_errors() {
    let src = r#"
        macro bad { ($x:expr) => ( ${index()} ); }
        fn main() { let a = bad!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_transcribe_error(&errors, "repetition context"),
        "expected transcribe error for index outside repetition, got {:?}",
        errors
    );
}

#[test]
fn len_outside_repetition_errors() {
    let src = r#"
        macro bad { ($x:expr) => ( ${len()} ); }
        fn main() { let a = bad!(1); }
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(
        has_transcribe_error(&errors, "repetition context"),
        "expected transcribe error for len outside repetition, got {:?}",
        errors
    );
}

// ============================================================
// Integration with existing features
// ============================================================

#[test]
fn count_allows_vec_like_preallocation_pattern() {
    let src = r#"
        macro vec {
            ($($x:expr),*) => ({
                let mut v = Vec::with_capacity(${count(x)});
                $( v.push($x); )*
                v
            });
        }
        fn main() { let a = vec!(1, 2, 3, 4); }
    "#;
    let (program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
    let init = let_init(&main_body(&program).statements[0].kind);
    assert!(matches!(init.kind, ExprKind::Block(_)));
}

#[test]
fn trailing_separator_in_attribute_args() {
    let src = r#"
        macro tagged {
            attr($($t:expr),*)($item:item) => { $item };
        }
        @tagged(1, 2, 3,)
        fn main() {}
    "#;
    let (_program, _interner, errors) = parse_and_expand(src);
    assert!(errors.is_empty(), "{:?}", errors);
}
