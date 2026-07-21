//! Lowering of AST items to HIR items.

use yelang_ast::item::{Enum as AstEnum, Struct as AstStruct};
use yelang_ast::{FnDef as AstFnDef, FnRefType, Item as AstItem, ItemKind as AstItemKind};

use crate::hir::core::{
    EnumDef, FieldDef, FnSig, GenericParam, Generics, ImplPolarity, Item, ItemKind, StructField,
    UseKind, UsePath, VariantData, VariantDef, Visibility, WhereClause, WherePredicate,
};
use crate::ids::{DefId, HirTyId};
use crate::lowering::LoweringContext;
use yelang_resolve::lang_items::LangItem;

/// Allocate a synthetic `DefId` for an AST item that name resolution did not
/// assign a definition to (e.g. inline modules, certain impl blocks).
fn allocate_item_def_id(ctx: &mut LoweringContext, item: &AstItem) -> DefId {
    use crate::lowering::context::{item_def_kind, item_name};
    let name = item_name(item).unwrap_or_else(|| match &item.kind {
        AstItemKind::Impl(_) => ctx.interner.get_or_intern("<impl>"),
        AstItemKind::Use(_) => ctx.interner.get_or_intern("<use>"),
        _ => ctx.interner.get_or_intern("<item>"),
    });
    let kind = item_def_kind(&item.kind).unwrap_or(yelang_resolve::DefKind::Module);
    ctx.allocate_synthetic_def(name, item.span, kind, item.visibility.clone())
}

/// Lower a single AST item into HIR.
pub fn lower_item(ctx: &mut LoweringContext, item: &AstItem) -> Option<DefId> {
    // Try to reuse the DefId assigned during name resolution.
    let def_id = crate::lowering::context::lookup_item_def_id(ctx, item)
        .unwrap_or_else(|| allocate_item_def_id(ctx, item));
    let prev_owner = ctx.current_owner;
    let prev_module = ctx.current_module;
    ctx.current_owner = def_id;

    let kind = match &item.kind {
        AstItemKind::Fn(f) => lower_fn_item(ctx, f, def_id),
        AstItemKind::Struct(s) => lower_struct_item(ctx, s, def_id),
        AstItemKind::Enum(e) => lower_enum_item(ctx, e, def_id),
        AstItemKind::Trait(t) => lower_trait_item(ctx, t, def_id),
        AstItemKind::Impl(i) => lower_impl_item(ctx, i, def_id),
        AstItemKind::TypeAlias(ta) => lower_type_alias_item(ctx, ta, def_id),
        AstItemKind::Const(c) => lower_const_item(ctx, c, def_id),
        AstItemKind::Static(s) => lower_static_item(ctx, s, def_id),
        AstItemKind::Module(m) => lower_module_item(ctx, m, def_id),
        AstItemKind::Use(u) => lower_use_item(ctx, u, def_id),
    };

    let hir_item = Item {
        def_id,
        ident: match &item.kind {
            AstItemKind::Fn(f) => f.name,
            AstItemKind::Struct(s) => s.name,
            AstItemKind::Enum(e) => e.name,
            AstItemKind::Trait(t) => t.name,
            AstItemKind::TypeAlias(ta) => ta.name,
            AstItemKind::Const(c) => c.name,
            AstItemKind::Static(s) => s.name,
            AstItemKind::Module(m) => m.name,
            AstItemKind::Impl(_) | AstItemKind::Use(_) => {
                // Use a synthetic name for impls and uses.
                yelang_ast::Ident::new(ctx.interner.get_or_intern("<item>"), item.span)
            }
        },
        kind,
        vis: item.visibility.clone(),
        attrs: item.attributes.clone(),
        span: item.span,
    };

    ctx.crate_hir.items.insert(def_id, Some(hir_item.clone()));

    // Expand built-in derives and attributes for this item.
    crate::derive::expand_item_derives(ctx, item, &hir_item);

    ctx.current_owner = prev_owner;
    ctx.current_module = prev_module;
    Some(def_id)
}

fn lower_fn_item(ctx: &mut LoweringContext, f: &AstFnDef, _def_id: DefId) -> ItemKind {
    let inputs: Vec<HirTyId> = f
        .sig
        .params
        .iter()
        .map(|p| crate::lowering::ty::lower_ty(ctx, &p.ty))
        .collect();
    let sig = lower_fn_sig(ctx, &f.sig, f.is_const, inputs.clone());
    let body_id = crate::lowering::body::lower_block_as_body(ctx, &f.body, &f.sig.params, &inputs);

    ItemKind::Fn {
        sig,
        body: body_id,
        generics: lower_generics(ctx, &f.generics),
    }
}

fn lower_fn_sig(
    ctx: &mut LoweringContext,
    sig: &yelang_ast::FnSig,
    is_const: bool,
    inputs: Vec<HirTyId>,
) -> FnSig {
    let output = match &sig.return_type {
        FnRefType::Type(ty) => crate::lowering::ty::lower_ty(ctx, ty),
        FnRefType::Default(span) => ctx
            .crate_hir
            .alloc_ty(crate::hir::ty::Ty::Tuple { tys: vec![] }, *span),
    };

    FnSig {
        inputs,
        output,
        is_async: sig.is_async,
        is_const,
        is_variadic: sig.is_variadic,
        abi: sig.abi.clone(),
        bound_vars: vec![],
    }
}

fn lower_struct_item(ctx: &mut LoweringContext, s: &AstStruct, _def_id: DefId) -> ItemKind {
    let data = match &s.fields {
        yelang_ast::StructFields::Named(fields) => VariantData::Struct {
            fields: fields
                .iter()
                .map(|f| FieldDef {
                    def_id: ctx.allocate_synthetic_def(
                        f.name.symbol,
                        f.span,
                        yelang_resolve::DefKind::Field,
                        f.visibility.clone(),
                    ),
                    ident: f.name,
                    ty: crate::lowering::ty::lower_ty(ctx, &f.ty),
                    span: f.span,
                    vis: f.visibility.clone(),
                    attrs: f.attributes.clone(),
                })
                .collect(),
        },
        yelang_ast::StructFields::Tuple(tys) => VariantData::Tuple {
            fields: tys
                .iter()
                .enumerate()
                .map(|(i, ty)| StructField {
                    def_id: ctx.allocate_synthetic_def(
                        ctx.interner.get_or_intern(&format!("{i}")),
                        ty.span,
                        yelang_resolve::DefKind::Field,
                        Visibility::Private,
                    ),
                    ty: crate::lowering::ty::lower_ty(ctx, ty),
                    span: ty.span,
                    vis: Visibility::Private,
                    attrs: vec![],
                })
                .collect(),
        },
        yelang_ast::StructFields::Unit => VariantData::Unit,
    };

    ItemKind::Struct {
        data,
        generics: lower_generics(ctx, &s.generics),
    }
}

fn lower_enum_item(ctx: &mut LoweringContext, e: &AstEnum, _def_id: DefId) -> ItemKind {
    let mut next_discriminant: u128 = 0;
    let variants: Vec<VariantDef> = e
        .variants
        .iter()
        .map(|v| {
            let data = match &v.kind {
                yelang_ast::VariantKind::Unit => VariantData::Unit,
                yelang_ast::VariantKind::Tuple(tys) => VariantData::Tuple {
                    fields: tys
                        .iter()
                        .enumerate()
                        .map(|(i, ty)| StructField {
                            def_id: ctx.allocate_synthetic_def(
                                ctx.interner.get_or_intern(&format!("{i}")),
                                ty.span,
                                yelang_resolve::DefKind::Field,
                                Visibility::Private,
                            ),
                            ty: crate::lowering::ty::lower_ty(ctx, ty),
                            span: ty.span,
                            vis: Visibility::Private,
                            attrs: vec![],
                        })
                        .collect(),
                },
                yelang_ast::VariantKind::Struct(fields) => VariantData::Struct {
                    fields: fields
                        .iter()
                        .map(|f| FieldDef {
                            def_id: ctx.allocate_synthetic_def(
                                f.name.symbol,
                                f.span,
                                yelang_resolve::DefKind::Field,
                                f.visibility.clone(),
                            ),
                            ident: f.name,
                            ty: crate::lowering::ty::lower_ty(ctx, &f.ty),
                            span: f.span,
                            vis: f.visibility.clone(),
                            attrs: f.attributes.clone(),
                        })
                        .collect(),
                },
            };
            let discriminant = match &v.discriminant {
                Some(expr) => {
                    next_discriminant = explicit_discriminant_value(ctx, expr)
                        .map(|v| v.saturating_add(1))
                        .unwrap_or(0);
                    crate::lowering::ty::lower_const_expr(ctx, expr, expr.span)
                }
                None => make_implicit_discriminant(ctx, v.span, &mut next_discriminant),
            };
            VariantDef {
                def_id: ctx.allocate_synthetic_def(
                    v.name.symbol,
                    v.span,
                    yelang_resolve::DefKind::EnumVariant,
                    // Variants inherit the parent enum's visibility. The AST
                    // variant node does not carry visibility directly, so we
                    // default to public; this path is only exercised for
                    // source variants that already have a resolved definition.
                    Visibility::Public(v.span),
                ),
                ident: v.name,
                data,
                discriminant: Some(discriminant),
                attrs: v.attributes.clone(),
                span: v.span,
            }
        })
        .collect();

    ItemKind::Enum {
        def: EnumDef {
            variants,
            span: e.name.span,
        },
        generics: lower_generics(ctx, &e.generics),
    }
}

/// Try to extract an integer value from an explicit discriminant expression so
/// that subsequent implicit discriminants can be inferred sequentially.
fn explicit_discriminant_value(ctx: &LoweringContext, expr: &yelang_ast::Expr) -> Option<u128> {
    match &expr.kind {
        yelang_ast::ExprKind::Literal(yelang_ast::Literal::Int(lit)) => {
            ctx.interner.resolve(&lit.value).parse().ok()
        }
        _ => None,
    }
}

/// Build an implicit enum discriminant as a literal integer constant and bump
/// the running counter.
fn make_implicit_discriminant(
    ctx: &mut LoweringContext,
    span: yelang_lexer::Span,
    next: &mut u128,
) -> crate::hir::ty::Const {
    let value = *next;
    *next = next.saturating_add(1);
    let sym = ctx.interner.get_or_intern(&value.to_string());
    crate::hir::ty::Const {
        kind: crate::hir::ty::ConstKind::Lit {
            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                value: sym,
                suffix: None,
            }),
        },
        span,
    }
}

fn lower_trait_item(
    ctx: &mut LoweringContext,
    t: &yelang_ast::item::Trait,
    def_id: DefId,
) -> ItemKind {
    let trait_def_id = def_id;
    let prev_self_type = ctx.self_type;
    ctx.self_type = Some(def_id);

    let items: Vec<crate::hir::core::TraitItem> = t
        .items
        .iter()
        .map(|item| {
            let ident = match &item.item {
                yelang_ast::TraitItemKind::Method(m) => m.segment,
                yelang_ast::TraitItemKind::Constant(c) => c.name,
                yelang_ast::TraitItemKind::AssociatedType(t) => t.name,
            };
            let kind = match &item.item {
                yelang_ast::TraitItemKind::Method(m) => {
                    let inputs: Vec<HirTyId> = m
                        .sig
                        .params
                        .iter()
                        .map(|p| crate::lowering::ty::lower_ty(ctx, &p.ty))
                        .collect();
                    crate::hir::core::TraitItemKind::Fn {
                        sig: lower_fn_sig(ctx, &m.sig, m.is_const, inputs.clone()),
                        generics: lower_generics(ctx, &m.generics),
                        default: m.body.as_ref().map(|body| {
                            crate::lowering::body::lower_block_as_body(
                                ctx,
                                body,
                                &m.sig.params,
                                &inputs,
                            )
                        }),
                    }
                }
                yelang_ast::TraitItemKind::Constant(c) => crate::hir::core::TraitItemKind::Const {
                    ty: crate::lowering::ty::lower_ty(ctx, &c.ty),
                    body: c
                        .value
                        .as_ref()
                        .map(|v| crate::lowering::body::lower_expr_as_body(ctx, v)),
                },
                yelang_ast::TraitItemKind::AssociatedType(ty) => {
                    crate::hir::core::TraitItemKind::Type {
                        bounds: ty
                            .bounds
                            .iter()
                            .map(|b| crate::lowering::ty::lower_trait_bound(ctx, b))
                            .collect(),
                        default: ty
                            .default
                            .as_ref()
                            .map(|ty| crate::lowering::ty::lower_ty(ctx, ty)),
                    }
                }
            };

            let def_kind = match &kind {
                crate::hir::core::TraitItemKind::Fn { .. } => yelang_resolve::DefKind::Fn,
                crate::hir::core::TraitItemKind::Const { .. } => yelang_resolve::DefKind::Const,
                crate::hir::core::TraitItemKind::Type { .. } => yelang_resolve::DefKind::TypeAlias,
            };
            let def_id = ctx.allocate_synthetic_def(
                ident.symbol,
                item.span,
                def_kind,
                Visibility::Public(item.span),
            );

            // If this is the `Target` associated type inside the lang-item `Deref`
            // trait, record its `DefId` so the type checker can build projection
            // goals (`<T as Deref>::Target`).
            if let Some(deref_trait) = ctx.resolved.lang_items.get(LangItem::Deref) {
                if deref_trait == trait_def_id && ctx.interner.resolve(&ident.symbol) == "Target" {
                    ctx.crate_hir
                        .lang_items
                        .insert(LangItem::DerefTarget, def_id);
                }
            }

            crate::hir::core::TraitItem {
                def_id,
                ident,
                kind,
                attrs: item.attributes.clone(),
                span: item.span,
            }
        })
        .collect();

    ctx.self_type = prev_self_type;

    let generics = lower_generics(ctx, &t.generics);
    let super_traits: Vec<crate::hir::core::TraitRef> = t
        .super_traits
        .iter()
        .map(|b| {
            let bound = crate::lowering::ty::lower_trait_bound(ctx, b);
            crate::hir::core::TraitRef {
                path: bound.path,
                args: bound.args,
                span: bound.span,
            }
        })
        .collect();

    ctx.crate_hir.traits.insert(
        def_id,
        Some(crate::hir::core::Trait {
            name: t.name,
            generics: generics.clone(),
            super_traits: super_traits.clone(),
            items: items.clone(),
            span: t.span,
        }),
    );

    ItemKind::Trait {
        items,
        generics,
        super_traits,
    }
}

fn lower_impl_item(
    ctx: &mut LoweringContext,
    i: &yelang_ast::item::Impl,
    _def_id: DefId,
) -> ItemKind {
    let impl_def_id = ctx.allocate_synthetic_def(
        ctx.interner.get_or_intern("<impl>"),
        i.span,
        yelang_resolve::DefKind::Impl,
        Visibility::Public(i.span),
    );
    let self_ty = crate::lowering::ty::lower_ty(ctx, &i.self_ty);
    let self_ty_def_id = ctx.crate_hir.ty(self_ty).and_then(|ty| match ty {
        crate::hir::ty::Ty::Path {
            res: crate::res::Res::Def { def_id },
            ..
        } => Some(*def_id),
        _ => None,
    });
    let prev_self_type = ctx.self_type;
    ctx.self_type = self_ty_def_id;

    let of_trait = i
        .trait_impl
        .as_ref()
        .map(|path| crate::hir::core::TraitRef {
            path: crate::lowering::expr::resolve_ast_path(ctx, path),
            args: crate::lowering::ty::lower_generic_args_from_path(ctx, path),
            span: path.span,
        });

    let items: Vec<crate::hir::core::ImplItem> = i
        .items
        .iter()
        .map(|item| {
            let ident = match &item.item {
                yelang_ast::ImplItemKind::Method(m) => m.name,
                yelang_ast::ImplItemKind::AssociatedType(at) => at.name,
                yelang_ast::ImplItemKind::Constant(c) => c.name,
            };
            let kind = match &item.item {
                yelang_ast::ImplItemKind::Method(m) => {
                    let inputs: Vec<HirTyId> = m
                        .sig
                        .params
                        .iter()
                        .map(|p| crate::lowering::ty::lower_ty(ctx, &p.ty))
                        .collect();
                    let sig = lower_fn_sig(ctx, &m.sig, m.is_const, inputs.clone());
                    let generics = lower_generics(ctx, &m.generics);
                    let body_id = crate::lowering::body::lower_block_as_body(
                        ctx,
                        &m.body,
                        &m.sig.params,
                        &inputs,
                    );
                    crate::hir::core::ImplItemKind::Fn { sig, generics, body: body_id }
                }
                yelang_ast::ImplItemKind::AssociatedType(at) => {
                    crate::hir::core::ImplItemKind::Type {
                        ty: crate::lowering::ty::lower_ty(ctx, &at.ty),
                    }
                }
                yelang_ast::ImplItemKind::Constant(c) => {
                    let ty = crate::lowering::ty::lower_ty(ctx, &c.ty);
                    let body_id = c
                        .value
                        .as_ref()
                        .map(|v| crate::lowering::body::lower_expr_as_body(ctx, v))
                        .unwrap_or_else(|| {
                            // No value provided: use unit as placeholder.
                            let unit_expr = yelang_ast::Expr {
                                kind: yelang_ast::ExprKind::Tuple(vec![]),
                                span: c.span,
                            };
                            crate::lowering::body::lower_expr_as_body(ctx, &unit_expr)
                        });
                    crate::hir::core::ImplItemKind::Const { ty, body: body_id }
                }
            };
            let def_kind = match &kind {
                crate::hir::core::ImplItemKind::Fn { .. } => yelang_resolve::DefKind::Fn,
                crate::hir::core::ImplItemKind::Type { .. } => yelang_resolve::DefKind::TypeAlias,
                crate::hir::core::ImplItemKind::Const { .. } => yelang_resolve::DefKind::Const,
            };
            let def_id = ctx.allocate_synthetic_def(
                ident.symbol,
                item.span,
                def_kind,
                item.visibility.clone(),
            );
            crate::hir::core::ImplItem {
                def_id,
                ident,
                kind,
                attrs: item.attributes.clone(),
                span: item.span,
                defaultness: match item.defaultness {
                    yelang_ast::item::Defaultness::Default => {
                        crate::hir::core::Defaultness::Default
                    }
                    yelang_ast::item::Defaultness::Final => crate::hir::core::Defaultness::Final,
                },
            }
        })
        .collect();

    let polarity = if i.is_negative {
        ImplPolarity::Negative
    } else {
        ImplPolarity::Positive
    };

    let impl_block = crate::hir::core::Impl {
        def_id: impl_def_id,
        generics: lower_generics(ctx, &i.generics),
        self_ty,
        of_trait,
        items,
        polarity,
        span: i.span,
    };
    ctx.crate_hir.impls.push(impl_block.clone());
    ctx.self_type = prev_self_type;

    ItemKind::Impl {
        items: impl_block.items,
        generics: impl_block.generics,
        self_ty: impl_block.self_ty,
        of_trait: impl_block.of_trait,
        polarity: impl_block.polarity,
    }
}

fn lower_type_alias_item(
    ctx: &mut LoweringContext,
    ta: &yelang_ast::item::TypeAlias,
    _def_id: DefId,
) -> ItemKind {
    ItemKind::TyAlias {
        ty: crate::lowering::ty::lower_ty(ctx, &ta.target),
        generics: lower_generics(ctx, &ta.generics),
    }
}

fn lower_const_item(
    ctx: &mut LoweringContext,
    c: &yelang_ast::item::Const,
    _def_id: DefId,
) -> ItemKind {
    let ty = crate::lowering::ty::lower_ty(ctx, &c.ty);
    let body_id = crate::lowering::body::lower_expr_as_body(ctx, &c.value);
    ItemKind::Const { ty, body: body_id }
}

fn lower_static_item(
    ctx: &mut LoweringContext,
    s: &yelang_ast::item::Static,
    _def_id: DefId,
) -> ItemKind {
    let ty = crate::lowering::ty::lower_ty(ctx, &s.ty);
    let body_id = crate::lowering::body::lower_expr_as_body(ctx, &s.value);
    ItemKind::Static {
        ty,
        mutability: if s.mutability {
            yelang_ast::Mutability::Mutable
        } else {
            yelang_ast::Mutability::Immutable
        },
        body: body_id,
    }
}

fn lower_module_item(ctx: &mut LoweringContext, m: &yelang_ast::ModDef, def_id: DefId) -> ItemKind {
    let mut item_ids = vec![];
    ctx.current_module = def_id;

    match &m.kind {
        yelang_ast::ModKind::Inline { items } => {
            for item in items {
                if let Some(id) = lower_item(ctx, item) {
                    item_ids.push(id);
                }
            }
        }
        yelang_ast::ModKind::External => {
            ctx.error(crate::lowering::err::LoweringError::UnsupportedAst {
                kind: "external module (mod name;)".to_string(),
                span: m.name.span,
            });
        }
    }

    ItemKind::Mod { items: item_ids }
}

fn lower_use_item(
    ctx: &mut LoweringContext,
    u: &yelang_ast::item::Use,
    _def_id: DefId,
) -> ItemKind {
    let (path, kind) = lower_use_tree(ctx, &u.tree);
    ItemKind::Use { path, kind }
}

/// Lower a top-level `use` tree into a primary path and a use kind.
fn lower_use_tree(
    ctx: &mut LoweringContext,
    tree: &yelang_ast::item::UseTree,
) -> (UsePath, UseKind) {
    use yelang_ast::item::UseTree;
    match tree {
        UseTree::Simple { path, span } => (make_use_path(ctx, path, *span, None), UseKind::Single),
        UseTree::Rename { path, alias, span } => (
            make_use_path(ctx, path, *span, Some(*alias)),
            UseKind::Single,
        ),
        UseTree::Glob { path, span } => (make_use_path(ctx, path, *span, None), UseKind::Glob),
        UseTree::Nested {
            prefix,
            items,
            span,
        } => {
            let prefix_path = make_use_path(ctx, prefix, *span, None);
            let nested: Vec<UsePath> = items
                .iter()
                .flat_map(|item| flatten_use_tree(ctx, prefix, item))
                .collect();
            (prefix_path, UseKind::Nested { items: nested })
        }
    }
}

/// Flatten a (possibly nested) use tree relative to a prefix into a list of
/// fully-qualified imported paths.
fn flatten_use_tree(
    ctx: &mut LoweringContext,
    prefix: &yelang_ast::Path,
    tree: &yelang_ast::item::UseTree,
) -> Vec<UsePath> {
    use yelang_ast::item::UseTree;
    match tree {
        UseTree::Simple { path, span } => vec![make_use_path(
            ctx,
            &combine_paths(prefix, path),
            *span,
            None,
        )],
        UseTree::Rename { path, alias, span } => vec![make_use_path(
            ctx,
            &combine_paths(prefix, path),
            *span,
            Some(*alias),
        )],
        UseTree::Glob { path, span } => vec![make_use_path(
            ctx,
            &combine_paths(prefix, path),
            *span,
            None,
        )],
        UseTree::Nested {
            prefix: inner_prefix,
            items,
            span: _,
        } => {
            let combined = combine_paths(prefix, inner_prefix);
            items
                .iter()
                .flat_map(|item| flatten_use_tree(ctx, &combined, item))
                .collect()
        }
    }
}

/// Build a `UsePath` by resolving an AST path.
fn make_use_path(
    ctx: &mut LoweringContext,
    path: &yelang_ast::Path,
    span: yelang_lexer::Span,
    rename: Option<yelang_ast::Ident>,
) -> UsePath {
    UsePath {
        res: crate::lowering::expr::resolve_ast_path(ctx, path),
        span,
        rename,
    }
}

/// Combine a prefix path with a suffix path, producing a new path whose
/// segments are the concatenation of the two.
fn combine_paths(prefix: &yelang_ast::Path, path: &yelang_ast::Path) -> yelang_ast::Path {
    let mut combined = prefix.clone();
    combined.segments.extend(path.segments.clone());
    combined.span = prefix.span.merge(path.span);
    combined
}

fn lower_generics(ctx: &mut LoweringContext, generics: &yelang_ast::Generics) -> Generics {
    Generics {
        params: generics
            .params
            .iter()
            .map(|p| lower_generic_param(ctx, p))
            .collect(),
        where_clause: generics
            .where_clause
            .as_ref()
            .map(|w| lower_where_clause(ctx, w)),
        span: generics.span,
    }
}

fn lower_generic_param(
    ctx: &mut LoweringContext,
    param: &yelang_ast::GenericParam,
) -> GenericParam {
    match param {
        yelang_ast::GenericParam::Type(tp) => {
            let span = tp.name.span();
            let def_id = ctx
                .resolved
                .generic_param_defs
                .get(&span)
                .copied()
                .unwrap_or_else(|| {
                    ctx.allocate_synthetic_def(
                        tp.name.symbol,
                        span,
                        yelang_resolve::DefKind::TypeParam,
                        Visibility::Public(span),
                    )
                });
            GenericParam::Type {
                def_id,
                name: tp.name,
                bounds: tp
                    .bounds
                    .iter()
                    .map(|b| crate::lowering::ty::lower_trait_bound(ctx, b))
                    .collect(),
                default: tp
                    .default
                    .as_ref()
                    .map(|ty| crate::lowering::ty::lower_ty(ctx, ty)),
                span,
            }
        }
        yelang_ast::GenericParam::Const(cp) => {
            let span = cp.name.span();
            let def_id = ctx
                .resolved
                .generic_param_defs
                .get(&span)
                .copied()
                .unwrap_or_else(|| {
                    ctx.allocate_synthetic_def(
                        cp.name.symbol,
                        span,
                        yelang_resolve::DefKind::ConstParam,
                        Visibility::Public(span),
                    )
                });
            GenericParam::Const {
                def_id,
                name: cp.name,
                ty: crate::lowering::ty::lower_ty(ctx, &cp.ty),
                default: cp
                    .default
                    .as_ref()
                    .map(|expr| crate::lowering::expr::lower_expr(ctx, expr)),
                span,
            }
        }
    }
}

fn lower_where_clause(ctx: &mut LoweringContext, clause: &yelang_ast::WhereClause) -> WhereClause {
    WhereClause {
        predicates: clause
            .predicates
            .iter()
            .map(|p| lower_where_predicate(ctx, p))
            .collect(),
        span: clause.span,
    }
}

fn lower_where_predicate(
    ctx: &mut LoweringContext,
    pred: &yelang_ast::WherePredicate,
) -> WherePredicate {
    match pred {
        yelang_ast::WherePredicate::TraitBound { ty, bounds } => WherePredicate::TraitBound {
            ty: crate::lowering::ty::lower_ty(ctx, ty),
            bounds: bounds
                .iter()
                .map(|b| crate::lowering::ty::lower_trait_bound(ctx, b))
                .collect(),
        },
        yelang_ast::WherePredicate::TypeEq { lhs, rhs } => WherePredicate::TypeEq {
            lhs: crate::lowering::ty::lower_ty(ctx, lhs),
            rhs: crate::lowering::ty::lower_ty(ctx, rhs),
        },
        yelang_ast::WherePredicate::ForAll {
            params,
            predicate,
            span,
        } => {
            // HRTB: `for<T> T: Clone` becomes a TraitBound with bound vars.
            // We lower the binder params into the type's generic params and
            // then lower the inner predicate.
            let bound_vars = crate::lowering::ty::lower_type_binder_params(ctx, params);
            match lower_where_predicate(ctx, predicate) {
                WherePredicate::TraitBound { ty, bounds } => {
                    let forall_ty = ctx.crate_hir.alloc_ty(
                        crate::hir::ty::Ty::ForAll {
                            params: bound_vars,
                            ty,
                        },
                        *span,
                    );
                    WherePredicate::TraitBound {
                        ty: forall_ty,
                        bounds,
                    }
                }
                other => other,
            }
        }
    }
}
