//! Exhaustive tests for AST item -> HIR item lowering.

use crate::hir::core::{ItemKind, VariantData};
use crate::lowering::lower_crate;
use crate::tests::common::{parse_program, resolved_with_defs, stub_resolved};

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

#[test]
fn lower_simple_fn() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    assert_eq!(crate_hir.items.len(), 1);
    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    assert!(matches!(&item.kind, ItemKind::Fn { .. }));
}

#[test]
fn lower_fn_with_params() {
    let src = "fn add(x: i32, y: i32) -> i32 { x + y }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { sig, .. } = &item.kind else {
        panic!("expected fn")
    };
    assert_eq!(sig.inputs.len(), 2);
}

#[test]
fn lower_fn_with_generics() {
    let src = "fn identity<T>(x: T) -> T { x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { generics, .. } = &item.kind else {
        panic!("expected fn")
    };
    assert_eq!(generics.params.len(), 1);
}

#[test]
fn lower_fn_with_where_clause() {
    let src = "fn clone<T>(x: T) -> T where T: Clone { x }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { generics, .. } = &item.kind else {
        panic!("expected fn")
    };
    assert!(generics.where_clause.is_some());
}

#[test]
fn lower_async_fn() {
    let src = "async fn foo() -> i32 { 42 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Fn { sig, .. } = &item.kind else {
        panic!("expected fn")
    };
    assert!(sig.is_async);
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[test]
fn lower_struct_named_fields() {
    let src = "struct Point { x: i32, y: i32 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Struct { data, .. } = &item.kind else {
        panic!("expected struct")
    };
    match data {
        VariantData::Struct { fields } => assert_eq!(fields.len(), 2),
        _ => panic!("expected named fields"),
    }
}

#[test]
fn lower_tuple_struct() {
    let src = "struct Pair(i32, string);";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Struct { data, .. } = &item.kind else {
        panic!("expected struct")
    };
    match data {
        VariantData::Tuple { fields } => assert_eq!(fields.len(), 2),
        _ => panic!("expected tuple fields"),
    }
}

#[test]
fn lower_unit_struct() {
    let src = "struct Nothing;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Struct { data, .. } = &item.kind else {
        panic!("expected struct")
    };
    assert!(matches!(data, VariantData::Unit));
}

#[test]
fn lower_struct_with_generics() {
    let src = "struct Wrapper<T> { value: T }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Struct { generics, .. } = &item.kind else {
        panic!("expected struct")
    };
    assert_eq!(generics.params.len(), 1);
}

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[test]
fn lower_enum_unit_variants() {
    let src = "enum Option { Some, None }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Enum { def, .. } = &item.kind else {
        panic!("expected enum")
    };
    assert_eq!(def.variants.len(), 2);
    assert!(matches!(def.variants[0].data, VariantData::Unit));
}

#[test]
fn lower_enum_tuple_variants() {
    let src = "enum Result { Ok(i32), Err(string) }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Enum { def, .. } = &item.kind else {
        panic!("expected enum")
    };
    assert_eq!(def.variants.len(), 2);
    match &def.variants[0].data {
        VariantData::Tuple { fields } => assert_eq!(fields.len(), 1),
        _ => panic!("expected tuple variant"),
    }
}

#[test]
fn lower_enum_struct_variants() {
    let src = "enum Shape { Circle { radius: f64 }, Rect { w: f64, h: f64 } }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Enum { def, .. } = &item.kind else {
        panic!("expected enum")
    };
    assert_eq!(def.variants.len(), 2);
    match &def.variants[1].data {
        VariantData::Struct { fields } => assert_eq!(fields.len(), 2),
        _ => panic!("expected struct variant"),
    }
}

#[test]
fn lower_enum_with_discriminant() {
    let src = "enum Color { Red = 1, Green = 2, Blue = 3 }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Enum { def, .. } = &item.kind else {
        panic!("expected enum")
    };
    assert!(def.variants[0].discriminant.is_some());
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

#[test]
fn lower_trait_with_methods() {
    let src = "trait Drawable { fn draw(&self); }";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Trait { items, .. } = &item.kind else {
        panic!("expected trait")
    };
    assert_eq!(items.len(), 1);
}

#[test]
fn lower_trait_with_const_and_type() {
    let src = r#"
        trait Config {
            const NAME: string;
            type Output;
        }
    "#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Trait { items, .. } = &item.kind else {
        panic!("expected trait")
    };
    assert_eq!(items.len(), 2);
}

// ---------------------------------------------------------------------------
// Impl blocks
// ---------------------------------------------------------------------------

#[test]
fn lower_inherent_impl() {
    let src = r#"
        struct Point { x: i32, y: i32 }
        impl Point {
            fn origin() -> Point { Point { x: 0, y: 0 } }
        }
    "#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    // Should have Point struct and impl item
    assert_eq!(crate_hir.items.len(), 2);
    let impl_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|i| matches!(i.kind, ItemKind::Impl { .. }))
        .expect("expected impl item");
    let ItemKind::Impl { self_ty, .. } = &impl_item.kind else {
        unreachable!()
    };
    let ty = crate_hir.tys.get(*self_ty).unwrap();
    assert!(matches!(ty, crate::hir::ty::Ty::Path { .. }));
}

#[test]
fn lower_self_struct_literal_in_impl() {
    let src = r#"
        struct Point { x: i32 }
        impl Point {
            fn origin() -> Self { Self { x: 0 } }
        }
    "#;
    let (program, interner) = parse_program(src);
    let point_sym = interner.get_or_intern("Point");
    let resolved = resolved_with_defs(&[(point_sym, yelang_resolve::DefKind::Struct)]);
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let impl_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|i| matches!(i.kind, ItemKind::Impl { .. }))
        .expect("expected impl item");
    let ItemKind::Impl { items, .. } = &impl_item.kind else {
        unreachable!()
    };
    let method = items
        .iter()
        .find(|i| i.ident.as_str(&interner) == "origin")
        .expect("expected origin");
    let crate::hir::core::ImplItemKind::Fn { body, .. } = &method.kind else {
        panic!("expected fn")
    };
    let body = crate_hir.bodies.get(*body).unwrap();
    let expr = crate_hir.exprs.get(body.value).unwrap();
    let crate::hir::core::Expr::Block { block, .. } = expr else {
        panic!("expected block")
    };
    let tail = crate_hir.exprs.get(block.expr.expect("expected tail expr")).unwrap();
    let crate::hir::core::Expr::Struct { path, .. } = tail else {
        panic!("expected struct literal, got {:?}", tail)
    };
    assert!(
        matches!(path, crate::res::Res::SelfTy { .. }),
        "expected SelfTy, got {:?}",
        path
    );
}

#[test]
fn lower_trait_impl() {
    let src = r#"
        trait Clone { fn clone(&self) -> Self; }
        struct Box<T> { value: T }
        impl<T> Clone for Box<T> where T: Clone {
            fn clone(&self) -> Self { Box { value: self.value } }
        }
    "#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let impl_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|i| matches!(i.kind, ItemKind::Impl { .. }))
        .expect("expected impl item");
    let ItemKind::Impl {
        of_trait, generics, ..
    } = &impl_item.kind
    else {
        unreachable!()
    };
    assert!(of_trait.is_some());
    assert_eq!(generics.params.len(), 1);
}

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

#[test]
fn lower_type_alias() {
    let src = "type MyInt = i32;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::TyAlias { ty, .. } = &item.kind else {
        panic!("expected type alias")
    };
    let ty_node = crate_hir.tys.get(*ty).unwrap();
    assert!(matches!(ty_node, crate::hir::ty::Ty::Path { .. }));
}

#[test]
fn lower_type_alias_with_generics() {
    let src = "type Pair<T> = (T, T);";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::TyAlias { generics, .. } = &item.kind else {
        panic!("expected type alias")
    };
    assert_eq!(generics.params.len(), 1);
}

// ---------------------------------------------------------------------------
// Const / Static
// ---------------------------------------------------------------------------

#[test]
fn lower_const_item() {
    let src = "const ANSWER: i32 = 42;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    assert!(matches!(item.kind, ItemKind::Const { .. }));
}

#[test]
fn lower_static_item() {
    let src = "static COUNTER: i32 = 0;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Static { mutability, .. } = &item.kind else {
        panic!("expected static")
    };
    assert!(matches!(mutability, yelang_ast::Mutability::Immutable));
}

#[test]
fn lower_mutable_static_item() {
    let src = "static mut COUNTER: i32 = 0;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Static { mutability, .. } = &item.kind else {
        panic!("expected static")
    };
    assert!(matches!(mutability, yelang_ast::Mutability::Mutable));
}

// ---------------------------------------------------------------------------
// Modules
// ---------------------------------------------------------------------------

#[test]
fn lower_inline_module() {
    let src = r#"
        mod inner {
            fn helper() {}
            struct Local;
        }
    "#;
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    let ItemKind::Mod { items } = &item.kind else {
        panic!("expected module")
    };
    assert_eq!(items.len(), 2);
}

// ---------------------------------------------------------------------------
// Use items
// ---------------------------------------------------------------------------

#[test]
fn lower_use_item() {
    let src = "use std::vec::Vec;";
    let (program, interner) = parse_program(src);
    let crate_hir = lower_crate(&program, &stub_resolved(), &interner);

    let item = crate_hir.items.values().find_map(|opt| opt.as_ref()).unwrap();
    assert!(matches!(item.kind, ItemKind::Use { .. }));
}
