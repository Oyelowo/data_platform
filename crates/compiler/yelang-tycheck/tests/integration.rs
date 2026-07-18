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
    let mut tcx = TyCtxt::with_string_interner(crate_hir, interner.clone());
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

fn assert_no_errors_named(diagnostics: &[Diagnostic], test_name: &str) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{}: expected no errors, got: {:?}",
        test_name,
        errors
    );
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
    assert_error_contains(&diagnostics, "trait bound not satisfied");
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

// ---------------------------------------------------------------------------
// Integer/float inference fallback at coercion sites
// ---------------------------------------------------------------------------

#[test]
fn integer_literal_coerces_to_annotated_i32() {
    let src = r#"
fn main() -> i32 {
    let x: i32 = 1;
    x
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "integer fallback");
}

#[test]
fn float_literal_coerces_to_annotated_f64() {
    let src = r#"
fn main() -> f64 {
    let y: f64 = 1.0;
    y
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "float fallback");
}

// ---------------------------------------------------------------------------
// Method dispatch: inherent priority and mutability
// ---------------------------------------------------------------------------

#[test]
fn inherent_method_takes_priority_over_trait_method() {
    let src = r#"
trait Noise {
    fn noise(&self) -> i32;
}
struct S {}
impl S {
    fn noise(&self) -> i32 { 1 }
}
impl Noise for S {
    fn noise(&self) -> i32 { 2 }
}
fn main() -> i32 {
    S {}.noise()
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "inherent priority");
}

#[test]
fn mut_self_method_requires_mutable_receiver() {
    let src = r#"
struct C { value: i32 }
impl C {
    fn inc(&mut self) {}
}
fn main() {
    let c = C { value: 0 };
    c.inc();
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "no method");
}

#[test]
fn mut_self_method_works_through_mutable_reference() {
    let src = r#"
struct C { value: i32 }
impl C {
    fn inc(&mut self) {}
}
fn use_c(c: &mut C) {
    c.inc();
}
fn main() {}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "mutable ref receiver");
}

// ---------------------------------------------------------------------------
// Deref coercion and method dispatch via the Deref lang item
// ---------------------------------------------------------------------------

#[test]
fn deref_trait_coerces_reference_to_target() {
    let src = r#"
@lang("deref")
trait Deref {
    type Target;
}
struct Inner { val: i32 }
struct Wrapper { inner: Inner }
impl Deref for Wrapper {
    type Target = Inner;
}
fn wants(r: &Inner) -> i32 {
    r.val
}
fn main() -> i32 {
    let w = Wrapper { inner: Inner { val: 3 } };
    wants(&w)
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "deref coercion");
}

#[test]
fn method_dispatches_through_deref_trait() {
    let src = r#"
@lang("deref")
trait Deref {
    type Target;
}
struct Inner { val: i32 }
impl Inner {
    fn value(&self) -> i32 { self.val }
}
struct Wrapper { inner: Inner }
impl Deref for Wrapper {
    type Target = Inner;
}
fn main() -> i32 {
    let w = Wrapper { inner: Inner { val: 7 } };
    (&w).value()
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "method via deref");
}
