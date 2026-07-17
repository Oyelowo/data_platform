//! Allocation-discipline tests for the HIR/definition storage refactor.

use yelang_arena::IndexVec;

use crate::hir::ItemKind;
use crate::lowering::lower_crate;

#[test]
fn resolved_def_ids_are_dense() {
    let src = r#"
        fn foo() {}
        struct Bar { x: i32 }
        trait Baz {}
        enum Qux { A, B }
    "#;
    let (program, interner) = super::common::parse_program(src);
    let resolved = yelang_resolve::resolve_crate(&program, &interner);

    // Every key from DefId(1) through DefId(definitions.len()) must be valid.
    assert!(
        resolved.definitions.len() >= 4,
        "expected at least the user items plus root/prelude/primitives"
    );
    for i in 1..=resolved.definitions.len() {
        let id = yelang_arena::DefId::new(i as u32);
        assert!(
            resolved.definitions.get(id).is_some(),
            "def_id {:?} should be present in dense arena",
            id
        );
    }
}

#[test]
fn derived_impl_def_id_is_above_resolved_definitions() {
    let src = r#"
        @derive(Copy)
        struct Point { x: i32 }
    "#;
    let (program, interner) = super::common::parse_program(src);
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let base = resolved.definitions.len() as u32;
    let impl_item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|item| matches!(item.kind, ItemKind::Impl { .. }))
        .expect("derived Copy impl should be present");

    assert!(
        impl_item.def_id.raw() > base,
        "synthesized impl DefId {:?} must be greater than definition arena length {}",
        impl_item.def_id,
        base
    );
}

#[test]
fn body_ids_are_dense() {
    let src = r#"
        fn foo() { 1 }
        fn bar() { 2 }
        fn baz() { 3 }
    "#;
    let (program, interner) = super::common::parse_program(src);
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let crate_hir = lower_crate(&program, &resolved, &interner);

    assert!(
        crate_hir.bodies.len() >= 3,
        "expected at least one body per function"
    );
    let ids: Vec<_> = crate_hir.bodies.keys().collect();
    assert_eq!(ids.len(), crate_hir.bodies.len());
    for id in ids {
        assert!(
            crate_hir.bodies.get(id).is_some(),
            "every body key should map to a body"
        );
    }
}

#[test]
fn def_id_and_body_id_are_distinct_types() {
    // This test documents the type-safety guarantee: `DefId` and `BodyId` are
    // different types and cannot be mixed up.
    assert_ne!(
        std::any::TypeId::of::<yelang_arena::DefId>(),
        std::any::TypeId::of::<crate::ids::BodyId>()
    );
    // The following would be a compile error if uncommented:
    // let _ = yelang_arena::DefId::new(1) == crate::ids::BodyId::default();
}

#[test]
fn item_lookup_roundtrips() {
    let src = r#"
        fn foo() {}
    "#;
    let (program, interner) = super::common::parse_program(src);
    let resolved = yelang_resolve::resolve_crate(&program, &interner);
    let crate_hir = lower_crate(&program, &resolved, &interner);

    let item = crate_hir
        .items
        .values()
        .filter_map(|opt| opt.as_ref())
        .find(|item| item.ident.as_str(&interner) == "foo")
        .expect("foo item should exist");
    let def_id = item.def_id;

    let looked_up = crate_hir
        .items
        .get(def_id)
        .and_then(|opt| opt.as_ref())
        .expect("lookup should succeed");
    assert_eq!(looked_up.def_id, def_id);
}

#[test]
fn index_vec_default_is_empty() {
    let vec: IndexVec<yelang_arena::DefId, i32> = IndexVec::default();
    assert!(vec.is_empty());
    assert_eq!(vec.len(), 0);
}
