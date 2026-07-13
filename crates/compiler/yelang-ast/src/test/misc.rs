use super::harness::*;

#[test]
fn test_string_interpolation() {
    assert_round_trip::<Expr>("\"Hello ${name}!\"");
}
#[test]
fn test_try_operator() {
    assert_round_trip::<Expr>("do_something()?;");
}
#[test]
fn test_type_cast_and_check() {
    assert_round_trip::<Expr>("x as i32;");
    assert_round_trip::<Expr>("x is String;");
}
#[test]
fn test_async_await() {
    assert_round_trip::<Expr>("async { foo().await };");
    assert_round_trip::<Expr>("async |value: i32| value + 1;");
}
