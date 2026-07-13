use super::harness::*;

#[test]
fn test_items_functions() {
    assert_round_trip::<crate::Item>("fn add(x: i32, y: i32) -> i32 { x + y };");
}
#[test]
fn test_items_structs() {
    // Named fields
    assert_round_trip::<crate::Item>("struct User { name: String, age: i32 };");
    // Tuple struct
    assert_round_trip::<crate::Item>("struct Point(i32, i32);");
}
#[test]
fn test_items_enums() {
    // Variants without data
    assert_round_trip::<crate::Item>("enum Color { Red, Green, Blue };");
    // Variants with data
    assert_round_trip::<crate::Item>("enum Result { Ok(i32), Err(String) };");
}
#[test]
fn test_items_impl() {
    assert_round_trip::<crate::Item>(
        "impl User { fn new(name: String) -> Self { User { name } } };",
    );
}
#[test]
fn test_items_trait() {
    assert_round_trip::<crate::Item>("trait Drawable { fn draw() -> string }");
}
#[test]
fn test_items_use() {
    assert_round_trip::<crate::Item>("use std::collections::HashMap;");
    assert_round_trip::<crate::Item>("use a::b::{c, d};");
}
#[test]
fn test_generics_and_where_clauses() {
    assert_round_trip::<FnDef>("fn foo(x: T) where T: Display { }");
    assert_round_trip::<FnDef>("fn foo<T: Clone>(x: T) where T: Display { }");
    assert_round_trip::<crate::Item>("fn foo<T: Clone>(x: T) where T: Display { }");
    assert_round_trip::<crate::Item>("struct Box<T> { inner: T }");
}
#[test]
fn test_attributes() {
    assert_round_trip::<crate::Item>("@derive(Debug) struct Foo;");
    let input = "@table(name = 'users') struct User { }";
    let mut interner = Interner::new();
    let mut tokens = TokenKind::tokenize(input, &mut interner).unwrap();
    let parsed: crate::Item = tokens.parse().unwrap();
    assert_eq!(parsed.attributes.len(), 1);
    let attr = &parsed.attributes[0];
    assert_eq!(attr.path.len(), 1);
    assert_eq!(attr.path[0].as_str(&interner), "table");
    if let crate::item::AttributeArgs::Named(named_args) = &attr.args {
        assert_eq!(named_args.len(), 1);
        assert_eq!(named_args[0].name.as_str(&interner), "name");
        // The value is an Expr, so we need to check it differently
    }
    assert_round_trip::<crate::Item>(input);
}

#[test]
fn test_impl_method_match_tail_before_next_attributed_method() {
    let input = r#"
        impl<T> [T] {
            @lang("collection_exactly_one")
            pub fn exactly_one(self) -> Result<T, CardinalityError> {
                match self {
                    [x] => Result::Ok(x),
                    [] => Result::Err(CardinalityError::Empty),
                    _ => Result::Err(CardinalityError::MoreThanOne),
                }
            }

            @lang("collection_expect_one")
            pub fn expect_one(self) -> T {
                match self {
                    [x] => {
                        x
                    },
                    _ => panic_cardinality_exactly_one(),
                }
            }
        }
    "#;

    assert_round_trip::<crate::Item>(input);
}

#[test]
fn test_visibility() {
    assert_round_trip::<crate::Item>("pub fn a() {}");
    assert_round_trip::<crate::Item>("pub fn a(a: i32) {}");
    assert_round_trip::<crate::Item>("pub fn a(a: i32,) {}");
    assert_round_trip::<crate::Item>("pub fn a(a: i32, b: String) {}");
    assert_round_trip::<crate::Item>("pub(crate) struct B;");
}
