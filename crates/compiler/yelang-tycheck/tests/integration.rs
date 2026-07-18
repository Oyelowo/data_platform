/*! End-to-end type-checker integration tests.
 *
 * Each test parses source text, runs name resolution, lowers to HIR, and runs
 * the full type checker. Tests assert either that no diagnostics are emitted or
 * that a specific diagnostic is produced.
 */

use yelang_ast::Program;
use yelang_hir::lower_crate;
use yelang_interner::Interner;
use yelang_tycheck::diagnostics::{Diagnostic, Severity};
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::type_check_crate;

/// Parse, resolve, lower, and type-check `src`. Returns the `TyCtxt` and any
/// diagnostics emitted by the type checker. Panics if lexing/parsing fails or
/// if name resolution produces unexpected errors.
fn type_check_src(src: &str) -> (TyCtxt, Vec<Diagnostic>) {
    let interner = Interner::new();
    let mut stream = yelang_lexer::TokenKind::tokenize(src, &interner).expect("tokenize");
    let program = stream.parse::<Program>().expect("parse program");

    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    assert!(
        resolved.errors.is_empty(),
        "name resolution produced errors: {:?}",
        resolved.errors
    );

    let crate_hir = lower_crate(&program, &resolved, &interner);
    let mut tcx = TyCtxt::new(crate_hir);
    let diagnostics = type_check_crate(&mut tcx);
    (tcx, diagnostics)
}

fn assert_no_errors(diagnostics: &[Diagnostic]) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

fn assert_error(diagnostics: &[Diagnostic]) -> &Diagnostic {
    diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .expect("expected at least one error diagnostic")
}

fn assert_error_contains<'a>(diagnostics: &'a [Diagnostic], needle: &str) -> &'a Diagnostic {
    let err = assert_error(diagnostics);
    assert!(
        err.message.contains(needle),
        "expected error message to contain {:?}, got {:?}",
        needle,
        err.message
    );
    err
}

#[test]
fn valid_integer_function_has_no_errors() {
    let src = "fn main() -> i32 { 42 }";
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors(&diagnostics);
}

#[test]
fn return_type_mismatch_is_reported() {
    let src = "fn main() -> i32 { true }";
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn missing_field_is_reported() {
    let src = r#"
struct Point { x: i32, y: i32 }
fn main() -> i32 {
    let p = Point { x: 1, y: 2 };
    p.z
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "no field");
}

#[test]
fn trait_not_implemented_is_reported() {
    let src = r#"
trait Show {
    fn show(self);
}
fn main() {
    let x = 1;
    x.show();
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "trait not implemented");
}

#[test]
fn never_coerces_to_any_type() {
    let src = r#"
fn die() -> ! {
    loop {}
}
fn main() -> i32 {
    die()
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors(&diagnostics);
}

#[test]
fn fn_item_coerces_to_fn_ptr() {
    let src = r#"
fn inc(x: i32) -> i32 { x + 1 }
fn main() -> i32 {
    let f: fn(i32) -> i32 = inc;
    f(1)
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors(&diagnostics);
}

#[test]
fn if_condition_must_be_bool() {
    let src = "fn main() { if 1 { } }";
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn if_branches_must_unify() {
    let src = "fn main() -> i32 { if true { 1 } else { true } }";
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn call_argument_count_mismatch_is_reported() {
    let src = r#"
fn f(x: i32) -> i32 { x }
fn main() -> i32 { f() }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "argument count mismatch");
}

#[test]
fn call_argument_type_mismatch_is_reported() {
    let src = r#"
fn f(x: i32) -> i32 { x }
fn main() -> i32 { f(true) }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}
