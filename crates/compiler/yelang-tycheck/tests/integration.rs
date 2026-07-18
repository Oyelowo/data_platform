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

// -----------------------------------------------------------------------------
// Query expressions and array selectors
// -----------------------------------------------------------------------------

#[test]
fn query_select_scalar_projection() {
    let src = r#"
struct User { id: i32 }
fn main() -> i32 {
    select 1 from users@u:User
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query scalar projection");
}

#[test]
fn query_select_array_projection() {
    let src = r#"
struct User { id: i32 }
fn main() -> Array<i32> {
    select users@u[*].id from users@u:User
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query array projection");
}

#[test]
fn query_select_with_where() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> Array<i32> {
    select users@u[*].id from users@u:User where u.age > 18
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query where clause");
}

#[test]
fn query_where_must_be_bool() {
    let src = r#"
struct User { id: i32 }
fn main() -> Array<i32> {
    select users@u[*].id from users@u:User where u.id
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn array_builtin_len() {
    let src = r#"
struct User { id: i32 }
fn main() -> usize {
    let users: Array<User> = get_users();
    len(users)
}
fn get_users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array builtin len");
}

#[test]
fn array_builtin_is_empty() {
    let src = r#"
struct User { id: i32 }
fn main() -> bool {
    let users: Array<User> = get_users();
    users.is_empty()
}
fn get_users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array builtin is_empty");
}

#[test]
fn array_selector_map() {
    let src = r#"
struct User { id: i32 }
fn main() -> Array<i32> {
    users@u[*].id
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array selector map");
}

#[test]
fn array_selector_filter() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> Array<User> {
    users@u[where u.age > 18]
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array selector filter");
}

#[test]
fn array_literal_produces_dynamic_array() {
    let src = r#"
fn main() -> Array<i32> {
    [1, 2, 3]
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array literal dynamic");
}

#[test]
fn query_from_modifiers_filter_order_range() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> Array<i32> {
    select users@u[*].id from (users@u:User where u.age > 18 order by u.id asc range ..10)
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query from modifiers");
}

#[test]
fn query_top_level_where_order_range() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> Array<i32> {
    select users@u[*].id from users@u:User where u.age > 18 order by u.id desc range 1..10
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query top level tails");
}

#[test]
fn query_projection_object() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> _ {
    select users@u[*].{ id: u.id, age: u.age } from users@u:User
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "query object projection");
}

#[test]
fn nested_selector_field_access() {
    let src = r#"
struct Address { city: i32 }
struct User { id: i32, address: Address }
fn main() -> Array<i32> {
    users@u[*].address.city
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "nested selector field access");
}

#[test]
fn nested_selector_with_filter() {
    let src = r#"
struct Address { city: i32 }
struct User { id: i32, age: i32, address: Address }
fn main() -> Array<i32> {
    users@u[where u.age > 18].address.city
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "nested selector filter");
}

#[test]
fn array_selector_chained_map_and_filter() {
    let src = r#"
struct User { id: i32, age: i32 }
fn main() -> Array<i32> {
    users@u[*].id@x[where x > 0]
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array selector chained map filter");
}

#[test]
fn array_selector_flatten_nested_arrays() {
    let src = r#"
struct Matrix { rows: Array<Array<i32>> }
fn main() -> _ {
    matrix@u[*].rows@r[**]
}
fn matrix() -> Array<Matrix> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array selector flatten");
}

#[test]
fn array_builtin_count() {
    let src = r#"
struct User { id: i32 }
fn main() -> usize {
    let users: Array<User> = get_users();
    count(users)
}
fn get_users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array builtin count");
}

#[test]
fn array_builtin_any_all() {
    let src = r#"
struct User { id: i32 }
fn main() -> bool {
    let users: Array<User> = get_users();
    users.any(|u| u.id > 0) && users.all(|u| u.id > 0)
}
fn get_users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array builtin any all");
}

#[test]
fn len_requires_array() {
    let src = r#"
fn main() -> usize {
    len(1)
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "expected an array type");
}

#[test]
fn is_empty_requires_array() {
    let src = r#"
fn main() -> bool {
    (1).is_empty()
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "expected an array type");
}

#[test]
fn field_access_on_array_requires_mapping() {
    let src = r#"
struct User { id: i32 }
fn main() -> i32 {
    let users: Array<User> = get_users();
    users.id
}
fn get_users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "no field");
}

#[test]
fn selector_filter_must_be_bool() {
    let src = r#"
struct User { id: i32 }
fn main() -> Array<User> {
    users@u[where u.id]
}
fn users() -> Array<User> { [] }
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

// -----------------------------------------------------------------------------
// Fixed-size arrays `[T; N]` and repeat expressions `[value; count]`
// -----------------------------------------------------------------------------

#[test]
fn fixed_size_array_literal_typechecks() {
    let src = r#"
fn main() -> [i32; 3] {
    [1; 3]
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "fixed-size array literal");
}

#[test]
fn fixed_size_array_repeat_requires_usize_count() {
    let src = r#"
fn main() -> [i32; 3] {
    [1; true]
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn fixed_size_array_mismatch_reports_error() {
    let src = r#"
fn main() -> [i32; 3] {
    [true; 3]
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn array_repeat_return_type_inferred() {
    let src = r#"
fn main() -> _ {
    [1; 3]
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "array repeat return inference");
}

// -----------------------------------------------------------------------------
// Mutation query expressions: create, update, upsert, delete, link, unlink
// -----------------------------------------------------------------------------

#[test]
fn create_query_returns_projection() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> str {
    create { user@u:User { id: 1, name: 'Alice' } return u.name }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "create query return projection");
}

#[test]
fn create_query_return_type_inferred() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> _ {
    create { user@u:User { id: 1, name: 'Alice' } return u.name }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "create query inferred return");
}

#[test]
fn update_query_set_returns_projection() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> str {
    update { users@u:User set u.name = 'Bob' where u.id == 1 return u.name }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "update query set");
}

#[test]
fn update_query_merge_returns_projection() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> i32 {
    update { users@u:User { name: 'Bob' } where u.id == 1 return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "update query merge");
}

#[test]
fn upsert_query_with_conflict_clause() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> i32 {
    upsert { user@u:User { id: 1, name: 'Alice' }
        on conflict (id) merge
        return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "upsert query conflict");
}

#[test]
fn delete_query_returns_projection() {
    let src = r#"
struct User { id: i32, name: str }
fn main() -> i32 {
    delete { users@u:User where u.id == 1 return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "delete query");
}

#[test]
fn create_link_query_returns_edge_field() {
    let src = r#"
struct User { id: i32 }
struct Follows { since: i32 }
fn main() -> i32 {
    link { (users@u:User) -> [follows@f:Follows { since: 2024 }] -> (friends@v:User)
        return f.since }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "create link query");
}

#[test]
fn unlink_query_returns_edge_field() {
    let src = r#"
struct User { id: i32 }
struct Follows { since: i32 }
fn main() -> i32 {
    unlink { (users@u:User) -> [follows@f:Follows] -> (friends@v:User)
        return f.since }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "unlink query");
}

#[test]
fn mutation_query_where_must_be_bool() {
    let src = r#"
struct User { id: i32 }
fn main() -> i32 {
    delete { users@u:User where u.id return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}

#[test]
fn create_query_data_fields_typecheck() {
    let src = r#"
struct User { id: i32 }
fn main() -> i32 {
    create { user@u:User { id: 1 } return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_no_errors_named(&diagnostics, "create data fields");
}

#[test]
fn create_query_data_field_type_mismatch() {
    let src = r#"
struct User { id: i32 }
fn main() -> i32 {
    create { user@u:User { id: "not an int" } return u.id }
}
"#;
    let (_tcx, diagnostics) = type_check_src(src);
    assert_error_contains(&diagnostics, "type mismatch");
}
