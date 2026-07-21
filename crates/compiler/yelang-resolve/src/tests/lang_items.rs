use crate::lang_items::LangItem;
use crate::tests::parse_program;
use crate::*;

// ---------------------------------------------------------------------------
// Primitive lang-item seeding
// ---------------------------------------------------------------------------

#[test]
fn primitives_are_seeded_as_lang_items() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    // Every primitive should be present in the registry.
    let primitives = [
        LangItem::I8,
        LangItem::I16,
        LangItem::I32,
        LangItem::I64,
        LangItem::I128,
        LangItem::Isize,
        LangItem::U8,
        LangItem::U16,
        LangItem::U32,
        LangItem::U64,
        LangItem::U128,
        LangItem::Usize,
        LangItem::F32,
        LangItem::F64,
        LangItem::Bool,
        LangItem::Char,
        LangItem::Str,
    ];
    for p in &primitives {
        assert!(
            collector.lang_items.contains(*p),
            "primitive lang item {:?} should be seeded",
            p
        );
    }
}

#[test]
fn primitive_definitions_have_lang_item_field() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    let i32_def_id = collector
        .lang_items
        .get(LangItem::I32)
        .expect("i32 lang item");
    let def = collector
        .definitions
        .get(i32_def_id)
        .expect("i32 definition");
    assert_eq!(def.lang_item, Some(LangItem::I32));
    assert_eq!(interner.resolve(&def.name), "i32");
    assert!(matches!(def.kind, def_collector::DefKind::TypeAlias));
}

// ---------------------------------------------------------------------------
// `@lang` attribute registration
// ---------------------------------------------------------------------------

#[test]
fn lang_attribute_on_trait_registers_lang_item() {
    let src = r#"
        @lang("copy")
        trait Copy {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector.lang_items.contains(LangItem::Copy),
        "@lang(copy) trait should register Copy lang item"
    );
}

#[test]
fn lang_attribute_on_fn_registers_lang_item() {
    let src = r#"
        @lang("panic")
        fn panic() {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector.lang_items.contains(LangItem::Panic),
        "@lang(panic) fn should register Panic lang item"
    );
}

#[test]
fn lang_attribute_on_struct_registers_lang_item() {
    let src = r#"
        @lang("owned_box")
        struct Box<T> { value: T }
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector.lang_items.contains(LangItem::Box),
        "@lang(owned_box) struct should register Box lang item"
    );
}

#[test]
fn duplicate_lang_item_emits_error() {
    let src = r#"
        @lang("copy")
        trait Copy {}

        @lang("copy")
        trait Copy2 {}

        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::DuplicateLangItem { .. })),
        "duplicate @lang(copy) should emit DuplicateLangItem error"
    );
}

#[test]
fn duplicate_lang_item_primitive_and_attr() {
    // Primitives are seeded first; a user-defined @lang("i32") should be flagged.
    let src = r#"
        @lang("i32")
        trait MyI32 {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::DuplicateLangItem { .. })),
        "@lang(i32) colliding with seeded primitive should emit error"
    );
}

// ---------------------------------------------------------------------------
// Prelude lang items
// ---------------------------------------------------------------------------

#[test]
fn prelude_lang_items_are_registered() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    // Prelude defines Copy, Clone, Debug, etc. as lang items.
    let expected = [
        LangItem::Copy,
        LangItem::Clone,
        LangItem::Default,
        LangItem::Debug,
        LangItem::Display,
        LangItem::PartialEq,
        LangItem::PartialOrd,
        LangItem::OrdTrait,
        LangItem::Send,
        LangItem::Sync,
        LangItem::Sized,
        LangItem::Box,
        LangItem::Drop,
    ];
    for li in &expected {
        assert!(
            collector.lang_items.contains(*li),
            "prelude lang item {:?} should be registered",
            li
        );
    }
}

// ---------------------------------------------------------------------------
// Registry query API
// ---------------------------------------------------------------------------

#[test]
fn lang_items_get_by_name() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(collector.lang_items.get_by_name("i32").is_some());
    assert!(collector.lang_items.get_by_name("copy").is_some());
    assert!(collector.lang_items.get_by_name("nonexistent").is_none());
}

#[test]
fn lang_items_iter_contains_all() {
    let src = "fn main() {}";
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    let items: Vec<_> = collector.lang_items.iter().collect();
    // 17 primitives + 15 prelude lang items = 32 minimum
    assert!(
        items.len() >= 32,
        "expected at least 32 lang items, got {}",
        items.len()
    );
}

// ---------------------------------------------------------------------------
// Resolver carries lang items
// ---------------------------------------------------------------------------

#[test]
fn resolver_inherits_lang_items() {
    let src = r#"
        @lang("panic")
        fn panic() {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);

    // The resolved crate doesn't directly expose lang_items, but we can verify
    // no errors occurred for a valid program.
    assert!(
        resolved
            .errors
            .iter()
            .all(|e| !matches!(e, ResolutionError::DuplicateLangItem { .. })),
        "valid @lang usage should not produce duplicate errors"
    );
}

// ---------------------------------------------------------------------------
// Invalid / edge-case `@lang` uses
// ---------------------------------------------------------------------------

#[test]
fn unknown_lang_attribute_is_ignored() {
    let src = r#"
        @lang("not_a_real_lang_item")
        trait Foo {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    // Unknown lang items are silently ignored; no definition gets a lang_item tag.
    // We just verify no crash and no DuplicateLangItem error.
    assert!(
        !collector
            .errors
            .iter()
            .any(|e| matches!(e, ResolutionError::DuplicateLangItem { .. })),
        "unknown lang item should not produce duplicate error"
    );
}

#[test]
fn item_without_lang_attribute_has_none() {
    let src = r#"
        trait Foo {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    // Find the Foo trait definition.
    let foo_def = collector
        .definitions
        .values()
        .find(|d| interner.resolve(&d.name) == "Foo")
        .expect("Foo definition");
    assert_eq!(foo_def.lang_item, None);
}

#[test]
fn lang_queryable_trait_registers_lang_item() {
    let src = r#"
        @lang("queryable")
        trait Queryable {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector.lang_items.contains(LangItem::Queryable),
        "@lang(queryable) trait should register Queryable lang item"
    );
}

#[test]
fn lang_aggregate_trait_registers_lang_item() {
    let src = r#"
        @lang("aggregate")
        trait Aggregate {}
        fn main() {}
    "#;
    let (program, interner) = parse_program(src);
    let collector = def_collector::DefCollector::new(&interner).collect(&program);

    assert!(
        collector.lang_items.contains(LangItem::Aggregate),
        "@lang(aggregate) trait should register Aggregate lang item"
    );
}
