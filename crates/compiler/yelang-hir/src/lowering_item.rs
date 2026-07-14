//! Lowering of AST items to HIR items.

use yelang_ast::{
    Item as AstItem, ItemKind as AstItemKind,
    Struct as AstStruct, Enum as AstEnum,
    FnDef as AstFnDef, FnRefType, Param as AstParam,
    Mutability,
};
use yelang_lexer::Span;

use crate::ids::{DefId, BodyId};
use crate::hir::{
    EnumDef, FieldDef, FnSig, Generics, Item, ItemKind, StructField, VariantData,
    VariantDef, Visibility,
};
use crate::hir_ty::Ty;
use crate::lowering::LoweringContext;
use crate::lowering_err::LoweringError;
use crate::res::Res;

/// Lower a single AST item into HIR.
pub fn lower_item(ctx: &mut LoweringContext, item: &AstItem) -> Option<DefId> {
    let def_id = ctx.next_def_id();
    let prev_owner = ctx.current_owner;
    ctx.current_owner = def_id;

    let kind = match &item.kind {
        AstItemKind::Fn(f) => lower_fn_item(ctx, f, def_id),
        AstItemKind::Struct(s) => lower_struct_item(ctx, s, def_id),
        AstItemKind::Enum(e) => lower_enum_item(ctx, e, def_id),
        _ => {
            ctx.error(LoweringError::UnsupportedAst {
                kind: format!("{:?}", std::mem::discriminant(&item.kind)),
                span: item.span,
            });
            ctx.current_owner = prev_owner;
            return None;
        }
    };

    let hir_item = Item {
        def_id,
        ident: match &item.kind {
            AstItemKind::Fn(f) => f.name,
            AstItemKind::Struct(s) => s.name,
            AstItemKind::Enum(e) => e.name,
            _ => return None,
        },
        kind,
        vis: item.visibility.clone(),
        span: item.span,
    };

    ctx.crate_hir.items.insert(def_id, hir_item);
    ctx.current_owner = prev_owner;
    Some(def_id)
}

fn lower_fn_item(ctx: &mut LoweringContext, f: &AstFnDef, def_id: DefId) -> ItemKind {
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
                    vis: Visibility::Private,
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
                            vis: Visibility::Private,
                        })
                        .collect(),
                },
            };
            VariantDef {
                ident: v.name,
                data,
                discriminant: v.discriminant.as_ref().map(|expr| {
                    crate::hir_ty::Const {
                        kind: crate::hir_ty::ConstKind::Lit {
                            lit: yelang_lexer::Literal::Int(yelang_lexer::IntegerLit {
                                value: yelang_interner::Symbol::from(0u32),
                                suffix: None,
                            }),
                        },
                        span: expr.span,
                    }
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

fn lower_generics(
    _ctx: &mut LoweringContext,
    generics: &yelang_ast::Generics,
) -> Generics {
    Generics {
        params: vec![],
        where_clause: None,
        span: generics.span,
    }
}
