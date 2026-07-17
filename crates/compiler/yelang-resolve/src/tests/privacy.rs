use crate::tests::parse_program;
use crate::*;

#[test]
fn debug_pub_const() {
    let src = r#"
        mod foo {
            pub const BAR: i32 = 1;
        }
        fn main() { foo::BAR; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);

    println!("Errors: {:?}", resolved.errors);
    println!("\nModule tree:");
    for (id, node) in &resolved.module_tree.modules {
        println!("Module {:?} ({}):", id, node.name.as_str(&interner));
        for (ns, map) in &node.items {
            let ns_name = match ns {
                crate::namespaces::Namespace::Value => "Value",
                crate::namespaces::Namespace::Type => "Type",
            };
            for (name, def_id) in map {
                println!("  {}: {} -> {:?}", ns_name, name.as_str(&interner), def_id);
            }
        }
    }

    println!("\nDefinitions:");
    for (id, def) in &resolved.definitions {
        println!(
            "Def {:?}: {} (kind: {:?}, parent: {:?})",
            id,
            def.name.as_str(&interner),
            def.kind,
            def.parent
        );
    }
}

#[test]
fn pub_fn_accessible_from_parent_module() {
    let src = r#"
        mod foo {
            pub fn bar() {}
        }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn private_fn_not_accessible_from_sibling_module() {
    let src = r#"
        mod foo {
            fn bar() {}
        }
        mod baz {
            fn main() { foo::bar(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_fn_not_accessible_through_private_module_from_sibling() {
    let src = r#"
        mod foo {
            pub fn bar() {}
        }
        mod baz {
            fn main() { foo::bar(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // Module `foo` is private, so `foo::bar` is not accessible from `baz` (sibling).
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_fn_accessible_through_pub_module_from_sibling() {
    let src = r#"
        pub mod foo {
            pub fn bar() {}
        }
        mod baz {
            fn main() { foo::bar(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_crate_accessible_everywhere() {
    let src = r#"
        pub mod foo {
            pub(crate) fn bar() {}
        }
        mod baz {
            fn main() { foo::bar(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_super_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub(super) fn bar() {}
        }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_super_not_accessible_from_sibling() {
    let src = r#"
        mod foo {
            pub(super) fn bar() {}
        }
        mod baz {
            fn main() { foo::bar(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // `pub(super)` is only visible from parent, not from sibling `baz`.
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_self_same_as_private() {
    let src = r#"
        mod foo {
            pub(self) fn bar() {}
        }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // `pub(self)` is only visible within `foo` itself.
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_in_path_accessible_in_target_module() {
    let src = r#"
        mod foo {
            pub(in crate) fn bar() {}
        }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_in_path_not_accessible_outside_target() {
    let src = r#"
        mod foo {
            pub(in self) fn bar() {}
        }
        fn main() { foo::bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // `pub(in self)` is only visible within `foo`.
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn enum_variants_inherit_pub_visibility() {
    let src = r#"
        pub enum Option {
            Some(i32),
            None,
        }
        fn main() { Some(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn enum_variants_inherit_private_visibility() {
    let src = r#"
        mod foo {
            enum Option {
                Some(i32),
                None,
            }
        }
        fn main() { foo::Some(1); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // Private enum variants are not accessible from outside.
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_struct_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub struct Bar {}
        }
        fn main() { let x: foo::Bar; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn private_struct_not_accessible_from_sibling() {
    let src = r#"
        mod foo {
            struct Bar {}
        }
        mod baz {
            fn main() { let x: foo::Bar; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn import_of_private_item_fails() {
    let src = r#"
        mod foo {
            fn bar() {}
        }
        use foo::bar;
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn import_of_pub_item_succeeds() {
    let src = r#"
        mod foo {
            pub fn bar() {}
        }
        use foo::bar;
        fn main() { bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn glob_import_respects_privacy() {
    let src = r#"
        mod foo {
            pub fn bar() {}
            fn baz() {}
        }
        use foo::*;
        fn main() { bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // `baz` should not be imported because it's private, but `bar` should be.
    assert!(
        !resolved.errors.iter().any(|e| matches!(e, ResolutionError::NotFound { name, .. } if name.as_str(&interner) == "bar")),
        "bar should be found: {:?}",
        resolved.errors
    );
    // Calling baz() should fail because it's not imported.
    let src2 = r#"
        mod foo {
            pub fn bar() {}
            fn baz() {}
        }
        use foo::*;
        fn main() { baz(); }
    "#;
    let (program2, interner2) = parse_program(src2);
    let resolved2 = resolve_crate(&program2, &interner2);
    assert!(
        resolved2
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::NotFound { .. })),
        "expected NotFound for baz: {:?}",
        resolved2.errors
    );
}

#[test]
fn nested_pub_module_chain() {
    let src = r#"
        pub mod outer {
            pub mod inner {
                pub fn deep() {}
            }
        }
        fn main() { outer::inner::deep(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn private_module_breaks_chain() {
    let src = r#"
        pub mod outer {
            mod inner {
                pub fn deep() {}
            }
        }
        fn main() { outer::inner::deep(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    // `inner` is private, so `outer::inner::deep()` should fail.
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_const_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub const BAR: i32 = 1;
        }
        fn main() { foo::BAR; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn private_const_not_accessible_from_sibling() {
    let src = r#"
        mod foo {
            const BAR: i32 = 1;
        }
        mod baz {
            fn main() { foo::BAR; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::PrivacyError { .. })),
        "expected privacy error: {:?}",
        resolved.errors
    );
}

#[test]
fn pub_trait_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub trait Bar {}
        }
        fn main() { let x: dyn foo::Bar; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_type_alias_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub type Bar = i32;
        }
        fn main() { let x: foo::Bar = 1; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn pub_static_accessible_from_parent() {
    let src = r#"
        mod foo {
            pub static BAR: i32 = 1;
        }
        fn main() { foo::BAR; }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn descendant_can_access_ancestor_module() {
    // A module can access its ancestor modules regardless of visibility.
    let src = r#"
        mod outer {
            mod inner {
                pub fn deep() {}
            }
            fn main() { inner::deep(); }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn use_from_pub_module_of_pub_item_succeeds() {
    let src = r#"
        pub mod foo {
            pub fn bar() {}
        }
        use foo::bar;
        fn main() { bar(); }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}
