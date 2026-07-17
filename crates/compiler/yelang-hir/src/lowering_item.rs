//! Lowering of AST items to HIR items.

use yelang_ast::item::{Enum as AstEnum, Struct as AstStruct};
use yelang_ast::{
    FnDef as AstFnDef, FnRefType, Item as AstItem, ItemKind as AstItemKind,
};

use crate::hir::{
    EnumDef, FieldDef, FnSig, GenericParam, Generics, Item, ItemKind, StructField,
    VariantData, VariantDef, Visibility, WhereClause, WherePredicate,
};
use crate::hir_ty::Ty;
use crate::ids::DefId;
use crate::lowering::LoweringContext;

/// Lower a single AST item into HIR.
pub fn lower_item(ctx: &mut LoweringContext, item: &AstItem) -> Option<DefId> {
    // Try to reuse the DefId assigned during name resolution.
    let def_id =
        crate::lowering::lookup_item_def_id(ctx, item).unwrap_or_else(|| ctx.next_synthetic_def_id());
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
    let sig = lower_fn_sig(ctx, &f.sig);
    let body_id = crate::lowering_body::lower_block_as_body(ctx, &f.body, &sig.inputs);

    ItemKind::Fn {
        sig,
        body: body_id,
        generics: lower_generics(ctx, &f.generics),
    }
}

fn lower_fn_sig(ctx: &mut LoweringContext, sig: &yelang_ast::FnSig) -> FnSig {
    let inputs: Vec<Ty> = sig
        .params
        .iter()
        .map(|p| crate::lowering_ty::lower_ty(ctx, &p.ty))
        .collect();

    let output = match &sig.return_type {
        FnRefType::Type(ty) => crate::lowering_ty::lower_ty(ctx, ty),
        FnRefType::Default(span) => Ty {
            kind: crate::hir_ty::TyKind::Tuple { tys: vec![] },
            span: *span,
        },
    };

    FnSig {
        inputs,
        output,
        is_async: sig.is_async,
        is_const: false,
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
                    ident: f.name,
                    ty: crate::lowering_ty::lower_ty(ctx, &f.ty),
                    span: f.span,
                    vis: f.visibility.clone(),
                })
                .collect(),
        },
        yelang_ast::StructFields::Tuple(tys) => VariantData::Tuple {
            fields: tys
                .iter()
                .map(|ty| StructField {
                    ty: crate::lowering_ty::lower_ty(ctx, ty),
                    span: ty.span,
                    vis: Visibility::Private,
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
    let variants: Vec<VariantDef> = e
        .variants
        .iter()
        .map(|v| {
            let data = match &v.kind {
                yelang_ast::VariantKind::Unit => VariantData::Unit,
                yelang_ast::VariantKind::Tuple(tys) => VariantData::Tuple {
                    fields: tys
                        .iter()
                        .map(|ty| StructField {
                            ty: crate::lowering_ty::lower_ty(ctx, ty),
                            span: ty.span,
                            vis: Visibility::Private,
                        })
                        .collect(),
                },
                yelang_ast::VariantKind::Struct(fields) => VariantData::Struct {
                    fields: fields
                        .iter()
                        .map(|f| FieldDef {
                            ident: f.name,
                            ty: crate::lowering_ty::lower_ty(ctx, &f.ty),
                            span: f.span,
                            vis: f.visibility.clone(),
                        })
                        .collect(),
                },
            };
            VariantDef {
                ident: v.name,
                data,
                discriminant: v.discriminant.as_ref().map(|expr| crate::hir_ty::Const {
                    kind: crate::hir_ty::ConstKind::Lit {
                        lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                            value: yelang_interner::Symbol::from(0u32),
                            suffix: None,
                        }),
                    },
                    span: expr.span,
                }),
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

fn lower_trait_item(
    ctx: &mut LoweringContext,
    t: &yelang_ast::item::Trait,
    def_id: DefId,
) -> ItemKind {
    let prev_self_type = ctx.self_type;
    ctx.self_type = Some(def_id);

    let items: Vec<crate::hir::TraitItem> = t
        .items
        .iter()
        .map(|item| {
            let kind = match &item.item {
                yelang_ast::TraitItemKind::Method(m) => crate::hir::TraitItemKind::Fn {
                    sig: lower_fn_sig(ctx, &m.sig),
                    default: m
                        .body
                        .as_ref()
                        .map(|body| crate::lowering_body::lower_block_as_body(ctx, body, &[])),
                },
                yelang_ast::TraitItemKind::Constant(c) => crate::hir::TraitItemKind::Const {
                    ty: crate::lowering_ty::lower_ty(ctx, &c.ty),
                    body: c
                        .value
                        .as_ref()
                        .map(|v| crate::lowering_body::lower_expr_as_body(ctx, v)),
                },
                yelang_ast::TraitItemKind::AssociatedType(ty) => crate::hir::TraitItemKind::Type {
                    bounds: ty
                        .bounds
                        .iter()
                        .map(|b| crate::lowering_ty::lower_trait_bound(ctx, b))
                        .collect(),
                    default: ty
                        .default
                        .as_ref()
                        .map(|ty| crate::lowering_ty::lower_ty(ctx, ty)),
                },
            };
            let ident = match &item.item {
                yelang_ast::TraitItemKind::Method(m) => m.segment,
                yelang_ast::TraitItemKind::Constant(c) => c.name,
                yelang_ast::TraitItemKind::AssociatedType(t) => t.name,
            };
            crate::hir::TraitItem {
                ident,
                kind,
                span: item.span,
            }
        })
        .collect();

    ctx.self_type = prev_self_type;

    ItemKind::Trait {
        items,
        generics: lower_generics(ctx, &t.generics),
    }
}

fn lower_impl_item(
    ctx: &mut LoweringContext,
    i: &yelang_ast::item::Impl,
    _def_id: DefId,
) -> ItemKind {
    let self_ty = crate::lowering_ty::lower_ty(ctx, &i.self_ty);
    let self_ty_def_id = match &self_ty.kind {
        crate::hir_ty::TyKind::Path {
            res: crate::res::Res::Def { def_id },
            ..
        } => Some(*def_id),
        _ => None,
    };
    let prev_self_type = ctx.self_type;
    ctx.self_type = self_ty_def_id;

    let of_trait = i.trait_impl.as_ref().map(|path| crate::hir::TraitRef {
        path: crate::lowering_expr::resolve_ast_path(ctx, path),
        span: path.span,
    });

    let items: Vec<crate::hir::ImplItem> = i
        .items
        .iter()
        .map(|item| {
            let kind = match &item.item {
                yelang_ast::ImplItemKind::Method(m) => {
                    let sig = lower_fn_sig(ctx, &m.sig);
                    let body_id =
                        crate::lowering_body::lower_block_as_body(ctx, &m.body, &sig.inputs);
                    crate::hir::ImplItemKind::Fn { sig, body: body_id }
                }
                yelang_ast::ImplItemKind::AssociatedType(at) => crate::hir::ImplItemKind::Type {
                    ty: crate::lowering_ty::lower_ty(ctx, &at.ty),
                },
                yelang_ast::ImplItemKind::Constant(c) => {
                    let ty = crate::lowering_ty::lower_ty(ctx, &c.ty);
                    let body_id = c
                        .value
                        .as_ref()
                        .map(|v| crate::lowering_body::lower_expr_as_body(ctx, v))
                        .unwrap_or_else(|| {
                            // No value provided: use unit as placeholder.
                            let unit_expr = yelang_ast::Expr {
                                kind: yelang_ast::ExprKind::Tuple(vec![]),
                                span: c.span,
                            };
                            crate::lowering_body::lower_expr_as_body(ctx, &unit_expr)
                        });
                    crate::hir::ImplItemKind::Const { ty, body: body_id }
                }
            };
            let ident = match &item.item {
                yelang_ast::ImplItemKind::Method(m) => m.name,
                yelang_ast::ImplItemKind::AssociatedType(at) => at.name,
                yelang_ast::ImplItemKind::Constant(c) => c.name,
            };
            crate::hir::ImplItem {
                ident,
                kind,
                span: item.span,
                defaultness: match item.defaultness {
                    yelang_ast::item::Defaultness::Default => crate::hir::Defaultness::Default,
                    yelang_ast::item::Defaultness::Final => crate::hir::Defaultness::Final,
                },
            }
        })
        .collect();

    let impl_block = crate::hir::Impl {
        generics: lower_generics(ctx, &i.generics),
        self_ty,
        of_trait,
        items,
        span: i.span,
    };
    ctx.crate_hir.impls.push(impl_block.clone());
    ctx.self_type = prev_self_type;

    ItemKind::Impl {
        items: impl_block.items,
        generics: impl_block.generics,
        self_ty: impl_block.self_ty,
        of_trait: impl_block.of_trait,
    }
}

fn lower_type_alias_item(
    ctx: &mut LoweringContext,
    ta: &yelang_ast::item::TypeAlias,
    _def_id: DefId,
) -> ItemKind {
    ItemKind::TyAlias {
        ty: crate::lowering_ty::lower_ty(ctx, &ta.target),
        generics: lower_generics(ctx, &ta.generics),
    }
}

fn lower_const_item(
    ctx: &mut LoweringContext,
    c: &yelang_ast::item::Const,
    _def_id: DefId,
) -> ItemKind {
    let ty = crate::lowering_ty::lower_ty(ctx, &c.ty);
    let body_id = crate::lowering_body::lower_expr_as_body(ctx, &c.value);
    ItemKind::Const { ty, body: body_id }
}

fn lower_static_item(
    ctx: &mut LoweringContext,
    s: &yelang_ast::item::Static,
    _def_id: DefId,
) -> ItemKind {
    let ty = crate::lowering_ty::lower_ty(ctx, &s.ty);
    let body_id = crate::lowering_body::lower_expr_as_body(ctx, &s.value);
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

    if let yelang_ast::ModKind::Inline { items } = &m.kind {
        for item in items {
            if let Some(id) = lower_item(ctx, item) {
                item_ids.push(id);
            }
        }
    }

    ItemKind::Mod { items: item_ids }
}

fn lower_use_item(
    _ctx: &mut LoweringContext,
    u: &yelang_ast::item::Use,
    _def_id: DefId,
) -> ItemKind {
    ItemKind::Use {
        path: crate::hir::UsePath {
            res: crate::res::Res::Err,
            span: u.span,
        },
        kind: crate::hir::UseKind::Single,
    }
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
        yelang_ast::GenericParam::Type(tp) => GenericParam::Type {
            name: tp.name,
            bounds: tp
                .bounds
                .iter()
                .map(|b| crate::lowering_ty::lower_trait_bound(ctx, b))
                .collect(),
            default: tp
                .default
                .as_ref()
                .map(|ty| crate::lowering_ty::lower_ty(ctx, ty)),
            span: tp.name.span(),
        },
        yelang_ast::GenericParam::Const(cp) => GenericParam::Const {
            name: cp.name,
            ty: crate::lowering_ty::lower_ty(ctx, &cp.ty),
            default: cp
                .default
                .as_ref()
                .map(|expr| Box::new(crate::lowering_expr::lower_expr(ctx, expr))),
            span: cp.name.span(),
        },
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
            ty: crate::lowering_ty::lower_ty(ctx, ty),
            bounds: bounds
                .iter()
                .map(|b| crate::lowering_ty::lower_trait_bound(ctx, b))
                .collect(),
        },
        yelang_ast::WherePredicate::TypeEq { lhs, rhs } => WherePredicate::TypeEq {
            lhs: crate::lowering_ty::lower_ty(ctx, lhs),
            rhs: crate::lowering_ty::lower_ty(ctx, rhs),
        },
        yelang_ast::WherePredicate::ForAll {
            params,
            predicate,
            span,
        } => {
            // HRTB: `for<T> T: Clone` becomes a TraitBound with bound vars.
            // We lower the binder params into the type's generic params and
            // then lower the inner predicate.
            let bound_vars = crate::lowering_ty::lower_type_binder_params(ctx, params);
            match lower_where_predicate(ctx, predicate) {
                WherePredicate::TraitBound { ty, bounds } => WherePredicate::TraitBound {
                    ty: Ty {
                        kind: crate::hir_ty::TyKind::ForAll {
                            params: bound_vars,
                            ty: Box::new(ty),
                        },
                        span: *span,
                    },
                    bounds,
                },
                other => other,
            }
        }
    }
}
