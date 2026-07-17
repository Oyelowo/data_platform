use yelang_lexer::Span;
use yelang_macro_core::{ExpnData, ExpnKind, HygieneData, SyntaxContextId, Transparency};

use crate::{
    def_collector::DefCollector,
    namespaces::Namespace,
    rib::{Resolution, RibKind},
    scope::Resolver,
    tests::parse_program,
};

fn fresh_opaque_context(hygiene: &HygieneData, parent: SyntaxContextId) -> SyntaxContextId {
    let expn = hygiene.fresh_expn(ExpnData {
        parent: hygiene.root_expn(),
        call_site: Span::default(),
        def_site: Span::default(),
        kind: ExpnKind::Macro,
        desc: "opaque".to_string(),
    });
    hygiene.apply_mark(parent, expn, Transparency::Opaque)
}

fn fresh_mixed_context(hygiene: &HygieneData, parent: SyntaxContextId) -> SyntaxContextId {
    let expn = hygiene.fresh_expn(ExpnData {
        parent: hygiene.root_expn(),
        call_site: Span::default(),
        def_site: Span::default(),
        kind: ExpnKind::MacroRules,
        desc: "mixed".to_string(),
    });
    hygiene.apply_mark(parent, expn, Transparency::Mixed)
}

fn span_with_ctx(ctx: SyntaxContextId) -> Span {
    Span::default().with_syntax_context(ctx.raw())
}

#[test]
fn local_value_binding_visible_from_same_context() {
    let (program, interner) = parse_program("fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let hygiene = HygieneData::new();
    let mut resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    )
    .with_hygiene(&hygiene);

    let name = interner.get_or_intern("x");
    resolver.push_rib(RibKind::Block);
    resolver.value_ribs.last_mut().unwrap().insert(
        Namespace::Value,
        name,
        Resolution::Local { local_id: 1 },
        Span::default(),
    );

    assert!(
        resolver
            .resolve_name(Namespace::Value, name, Span::default())
            .is_some()
    );
}

#[test]
fn opaque_mark_hides_local_value_binding() {
    let (program, interner) = parse_program("fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let hygiene = HygieneData::new();
    let mut resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    )
    .with_hygiene(&hygiene);

    let name = interner.get_or_intern("x");
    let opaque = fresh_opaque_context(&hygiene, hygiene.root_syntax_context());

    resolver.push_rib(RibKind::Block);
    resolver.value_ribs.last_mut().unwrap().insert(
        Namespace::Value,
        name,
        Resolution::Local { local_id: 1 },
        Span::default(),
    );

    assert!(
        resolver
            .resolve_name(Namespace::Value, name, span_with_ctx(opaque))
            .is_none()
    );
}

#[test]
fn mixed_mark_hides_local_value_binding_but_not_local_type_binding() {
    let (program, interner) = parse_program("fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let hygiene = HygieneData::new();
    let mut resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    )
    .with_hygiene(&hygiene);

    let value_name = interner.get_or_intern("x");
    let type_name = interner.get_or_intern("T");
    let mixed = fresh_mixed_context(&hygiene, hygiene.root_syntax_context());

    resolver.push_rib(RibKind::Fn);
    resolver.value_ribs.last_mut().unwrap().insert(
        Namespace::Value,
        value_name,
        Resolution::Local { local_id: 1 },
        Span::default(),
    );
    resolver.type_ribs.last_mut().unwrap().insert(
        Namespace::Type,
        type_name,
        Resolution::Local { local_id: 2 },
        Span::default(),
    );

    assert!(
        resolver
            .resolve_name(Namespace::Value, value_name, span_with_ctx(mixed))
            .is_none()
    );
    assert!(
        resolver
            .resolve_name(Namespace::Type, type_name, span_with_ctx(mixed))
            .is_some()
    );
}

#[test]
fn module_item_visible_through_mixed_mark() {
    let (program, interner) = parse_program("fn helper() {} fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let hygiene = HygieneData::new();
    let resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    )
    .with_hygiene(&hygiene);

    let name = interner.get_or_intern("helper");
    let mixed = fresh_mixed_context(&hygiene, hygiene.root_syntax_context());

    assert!(
        resolver
            .resolve_name_in_module(
                resolver.current_module,
                Namespace::Value,
                name,
                span_with_ctx(mixed)
            )
            .is_some()
    );
}

#[test]
fn opaque_module_item_is_not_visible_through_self() {
    // An item span that carries the inner macro context cannot be seen from
    // the same inner context if an opaque mark separates it from the root.
    let (program, interner) = parse_program("fn helper() {} fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let hygiene = HygieneData::new();
    let mut resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    )
    .with_hygiene(&hygiene);

    let name = interner.get_or_intern("helper");
    let opaque = fresh_opaque_context(&hygiene, hygiene.root_syntax_context());

    // Pretend the item itself was introduced by the opaque macro by mutating
    // its recorded span to the inner context.
    let helper_def_id = resolver
        .module_tree
        .modules
        .get(&resolver.current_module)
        .unwrap()
        .items
        .get(&Namespace::Value)
        .unwrap()
        .get(&name)
        .copied()
        .unwrap();
    resolver.definitions.get_mut(&helper_def_id).unwrap().span = span_with_ctx(opaque);

    assert!(
        resolver
            .resolve_name_in_module(
                resolver.current_module,
                Namespace::Value,
                name,
                Span::default()
            )
            .is_none()
    );
}

#[test]
fn unknown_use_context_is_not_a_barrier() {
    // A span carrying an unknown syntax context should not prevent resolution
    // when no hygiene data is present.
    let (program, interner) = parse_program("fn helper() {} fn main() {}");
    let collector = DefCollector::new(&interner).collect(&program);
    let resolver = Resolver::new(
        &interner,
        collector.module_tree,
        collector.definitions,
        collector.prelude,
        collector.lang_items,
        collector.enum_variants,
    );

    let name = interner.get_or_intern("helper");
    let unknown = Span::default().with_syntax_context(999);

    assert!(
        resolver
            .resolve_name_in_module(resolver.current_module, Namespace::Value, name, unknown)
            .is_some()
    );
}
