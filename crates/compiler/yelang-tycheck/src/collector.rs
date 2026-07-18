/*! Type collection from HIR items.
 *
 * Walks HIR items, traits, and impl blocks and populates `TyCtxt` tables:
 * `item_types`, `fn_sigs`, `adt_defs`, `trait_defs`, `impl_defs`, and the
 * trait-to-impl index.
 */

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::Crate as HirCrate;
use yelang_hir::hir::adt::VariantData;
use yelang_hir::hir::core as hir;
use yelang_hir::hir::item::{Item, ItemKind};
use yelang_ty::generic::GenericArg;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{Predicate, TraitPredicate};
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::{AdtDef, Const, ConstId, ParamTy, PolyFnSig, Ty, TyId};

use crate::hir_ty_lower::lower_hir_ty_id;
use crate::lower_ctx::TyLowerCtxt;
use crate::tcx::{
    AdtDefData, AdtKind, FieldData, GenericParamData, GenericParamKind, GenericsData, ImplDefData,
    ImplItemDefData, TraitDefData, TraitItemDefData, TyCtxt,
};

/// Collect item signatures from the HIR crate into `tcx`.
pub fn collect_crate_types(tcx: &mut TyCtxt) {
    // Pre-collect HIR nodes so we can mutate `tcx` while iterating.
    let items: Vec<_> = tcx
        .crate_hir()
        .items
        .iter_enumerated()
        .filter_map(|(def_id, item)| item.as_ref().map(|i| (def_id, i.clone())))
        .collect();
    let traits: Vec<_> = tcx
        .crate_hir()
        .traits
        .iter_enumerated()
        .filter_map(|(def_id, tr)| tr.as_ref().map(|t| (def_id, t.clone())))
        .collect();
    let impls: Vec<_> = tcx.crate_hir().impls.iter().cloned().collect();

    // Collect items indexed by DefId.
    for (def_id, item) in items {
        collect_item(tcx, def_id, &item);
    }

    // Trait definitions are stored separately.
    for (def_id, tr) in traits {
        collect_trait(tcx, def_id, &tr);
    }

    // Impl blocks are stored separately.
    for imp in impls {
        collect_impl(tcx, &imp);
    }

    tcx.populate_solver_caches();
}

fn collect_item(tcx: &mut TyCtxt, def_id: DefId, item: &Item) {
    let kind = item.kind.clone();
    match &kind {
        ItemKind::Fn {
            sig,
            body: _,
            generics,
        } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let poly_sig = lower_fn_sig(&mut cx, sig);
            let fn_ty = cx.tcx.interner().mk_ty(Ty::FnDef(yelang_ty::ty::FnDef {
                def_id,
                args: identity_args(&cx, &generics_data.params),
            }));
            cx.tcx.fn_sigs.insert(def_id, poly_sig);
            cx.tcx.generics.insert(def_id, generics_data);
            cx.tcx.item_types.insert(def_id, fn_ty);
        }
        ItemKind::Struct { data, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let adt = collect_struct(&mut cx, def_id, item.ident, data, generics_data);
            cx.tcx.generics.insert(def_id, adt.generics.clone());
            let ty = cx.tcx.interner().mk_ty(Ty::Adt(
                AdtDef { def_id },
                identity_args(&cx, &adt.generics.params),
            ));
            cx.tcx.adt_defs.insert(def_id, adt);
            cx.tcx.item_types.insert(def_id, ty);
        }
        ItemKind::Enum { def, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let adt = collect_enum(&mut cx, def_id, item.ident, def, generics_data);
            cx.tcx.generics.insert(def_id, adt.generics.clone());
            let ty = cx.tcx.interner().mk_ty(Ty::Adt(
                AdtDef { def_id },
                identity_args(&cx, &adt.generics.params),
            ));
            cx.tcx.adt_defs.insert(def_id, adt);
            cx.tcx.item_types.insert(def_id, ty);
        }
        ItemKind::Trait {
            items: _,
            generics: _,
            super_traits: _,
        } => {
            // Trait definitions are collected from `hir.traits`. The generics
            // and items are stored in `trait_defs`.
            tcx.item_types
                .insert(def_id, tcx.interner().mk_ty(Ty::Error));
        }
        ItemKind::Impl { .. } => {
            // Impl blocks are not items with a type.
        }
        ItemKind::TyAlias { ty, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let alias_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.generics.insert(def_id, generics_data);
            cx.tcx.item_types.insert(def_id, alias_ty);
        }
        ItemKind::Const { ty, body: _ } => {
            let mut cx = CollectorCx::new(tcx, &[]);
            let const_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.item_types.insert(def_id, const_ty);
        }
        ItemKind::Static {
            ty,
            mutability: _,
            body: _,
        } => {
            let mut cx = CollectorCx::new(tcx, &[]);
            let static_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.item_types.insert(def_id, static_ty);
        }
        ItemKind::Mod { .. } | ItemKind::Use { .. } => {
            // No type.
        }
    }
}

fn collect_trait(tcx: &mut TyCtxt, def_id: DefId, tr: &hir::Trait) {
    let generics_data = lower_generics(tcx, &tr.generics);
    let mut cx = CollectorCx::new(tcx, &generics_data.params);

    // In a trait definition `Self` is an implicit type parameter. It is placed
    // after all explicit generic parameters so that explicit parameter indices
    // remain 0..n-1 and `Self` is index n. This lets a single substitution
    // replace both the explicit parameters and `Self` when instantiating a
    // trait method signature.
    let self_param_index = generics_data.params.len() as u32;
    let self_ty = cx.tcx.interner().mk_ty(Ty::Param(ParamTy {
        index: self_param_index,
        name: yelang_interner::Symbol::from(0),
    }));
    cx.self_ty = Some(self_ty);

    let supertraits: Vec<_> = tr
        .super_traits
        .iter()
        .filter_map(|t| lower_trait_ref(&mut cx, self_ty, t))
        .collect();

    let items: Vec<_> = tr
        .items
        .iter()
        .map(|item| {
            let kind = item.kind.clone();
            match &kind {
                hir::TraitItemKind::Fn { sig, default: _ } => TraitItemDefData::Fn {
                    def_id: item.def_id,
                    ident: item.ident,
                    sig: lower_fn_sig(&mut cx, sig),
                },
                hir::TraitItemKind::Const { ty, body: _ } => TraitItemDefData::Const {
                    def_id: item.def_id,
                    ident: item.ident,
                    ty: lower_hir_ty_id(*ty, &mut cx),
                },
                hir::TraitItemKind::Type { bounds, default } => TraitItemDefData::Type {
                    def_id: item.def_id,
                    ident: item.ident,
                    bounds: bounds
                        .iter()
                        .filter_map(|b| lower_trait_bound(&mut cx, self_ty, b))
                        .collect(),
                    default: default.map(|t| lower_hir_ty_id(t, &mut cx)),
                },
            }
        })
        .collect();

    cx.tcx.generics.insert(def_id, generics_data.clone());
    cx.tcx.trait_defs.insert(
        def_id,
        TraitDefData {
            def_id,
            ident: tr.name,
            generics: generics_data,
            supertraits,
            items,
        },
    );
    cx.tcx
        .item_types
        .insert(def_id, cx.tcx.interner().mk_ty(Ty::Error));
}

fn collect_impl(tcx: &mut TyCtxt, imp: &hir::Impl) {
    let def_id = imp.def_id;
    let generics_data = lower_generics(tcx, &imp.generics);
    let mut cx = CollectorCx::new(tcx, &generics_data.params);

    let self_ty = lower_hir_ty_id(imp.self_ty, &mut cx);
    cx.self_ty = Some(self_ty);

    let trait_ref = imp
        .of_trait
        .as_ref()
        .and_then(|t| lower_trait_ref(&mut cx, self_ty, t));

    let items: Vec<_> = imp
        .items
        .iter()
        .map(|item| {
            let kind = item.kind.clone();
            match &kind {
                hir::ImplItemKind::Fn { sig, body: _ } => ImplItemDefData::Fn {
                    def_id: item.def_id,
                    ident: item.ident,
                    sig: lower_fn_sig(&mut cx, sig),
                },
                hir::ImplItemKind::Const { ty, body: _ } => ImplItemDefData::Const {
                    def_id: item.def_id,
                    ident: item.ident,
                    ty: lower_hir_ty_id(*ty, &mut cx),
                },
                hir::ImplItemKind::Type { ty } => ImplItemDefData::Type {
                    def_id: item.def_id,
                    ident: item.ident,
                    ty: lower_hir_ty_id(*ty, &mut cx),
                },
            }
        })
        .collect();

    cx.tcx.generics.insert(def_id, generics_data.clone());
    let impl_id = cx.tcx.impl_defs.push(ImplDefData {
        id: crate::tcx::ImplDefId::new(1), // patched below; 1 is a valid placeholder
        def_id,
        trait_ref,
        self_ty,
        generics: generics_data,
        items,
    });
    cx.tcx.impl_defs[impl_id].id = impl_id;

    if let Some(tr) = trait_ref {
        cx.tcx
            .trait_impl_index
            .entry(tr.def_id)
            .or_default()
            .push(impl_id);
    }
}

fn collect_struct(
    cx: &mut CollectorCx<'_>,
    def_id: DefId,
    ident: yelang_ast::Ident,
    data: &VariantData,
    generics: GenericsData,
) -> AdtDefData {
    let fields: Vec<_> = variant_fields(data)
        .iter()
        .map(|&(field_def_id, field_ident, ty_id)| FieldData {
            def_id: field_def_id,
            ident: field_ident,
            ty: lower_hir_ty_id(ty_id, cx),
        })
        .collect();

    AdtDefData {
        def_id,
        kind: AdtKind::Struct,
        ident,
        variants: vec![crate::tcx::VariantData {
            def_id,
            ident,
            fields,
            discriminant: None,
        }],
        generics,
    }
}

fn collect_enum(
    cx: &mut CollectorCx<'_>,
    def_id: DefId,
    ident: yelang_ast::Ident,
    enum_def: &hir::EnumDef,
    generics: GenericsData,
) -> AdtDefData {
    let variants: Vec<_> = enum_def
        .variants
        .iter()
        .map(|v| {
            let fields: Vec<_> = variant_fields(&v.data)
                .iter()
                .map(|&(field_def_id, field_ident, ty_id)| FieldData {
                    def_id: field_def_id,
                    ident: field_ident,
                    ty: lower_hir_ty_id(ty_id, cx),
                })
                .collect();
            crate::tcx::VariantData {
                def_id: v.def_id,
                ident: v.ident,
                fields,
                discriminant: v.discriminant.as_ref().map(|c| lower_hir_const(cx.tcx, c)),
            }
        })
        .collect();

    AdtDefData {
        def_id,
        kind: AdtKind::Enum,
        ident,
        variants,
        generics,
    }
}

fn variant_fields(data: &VariantData) -> Vec<(DefId, yelang_ast::Ident, yelang_hir::ids::HirTyId)> {
    match data {
        VariantData::Struct { fields } => {
            fields.iter().map(|f| (f.def_id, f.ident, f.ty)).collect()
        }
        VariantData::Tuple { fields } => fields
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let ident = yelang_ast::Ident::new(
                    yelang_interner::Symbol::from(idx as u32),
                    yelang_lexer::Span::default(),
                );
                (f.def_id, ident, f.ty)
            })
            .collect(),
        VariantData::Unit => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Lowering helpers
// ---------------------------------------------------------------------------

fn lower_fn_sig(cx: &mut CollectorCx<'_>, sig: &hir::FnSig) -> PolyFnSig {
    let inputs: Vec<_> = sig
        .inputs
        .iter()
        .map(|t| GenericArg::Type(lower_hir_ty_id(*t, cx)))
        .collect();
    let return_ty_infer = matches!(
        cx.tcx.crate_hir().ty(sig.output),
        Some(yelang_hir::hir::ty::Ty::Infer)
    );
    let output = if return_ty_infer {
        cx.tcx.interner().mk_ty(Ty::Error)
    } else {
        lower_hir_ty_id(sig.output, cx)
    };
    PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs: cx.tcx.interner().mk_generic_args(&inputs),
            output,
            return_ty_infer,
        },
    }
}

fn lower_trait_ref(
    cx: &mut CollectorCx<'_>,
    self_ty: TyId,
    tr: &hir::TraitRef,
) -> Option<yelang_ty::predicate::TraitRef> {
    lower_trait_bound(
        cx,
        self_ty,
        &hir::TraitBound {
            path: tr.path.clone(),
            args: vec![],
            span: tr.span,
        },
    )
}

fn lower_trait_bound(
    cx: &mut CollectorCx<'_>,
    self_ty: TyId,
    bound: &hir::TraitBound,
) -> Option<yelang_ty::predicate::TraitRef> {
    if let yelang_hir::res::Res::Def { def_id } = bound.path {
        let mut args = vec![GenericArg::Type(self_ty)];
        for arg in &bound.args {
            match arg {
                yelang_hir::hir::ty::GenericArg::Type(ty_id) => {
                    args.push(GenericArg::Type(lower_hir_ty_id(*ty_id, cx)));
                }
                yelang_hir::hir::ty::GenericArg::Const(c) => {
                    args.push(GenericArg::Const(lower_hir_const(cx.tcx, c)));
                }
                yelang_hir::hir::ty::GenericArg::AssocBinding { .. } => {
                    // TODO: lower associated type bindings once the type system
                    // supports them.
                }
            }
        }
        let args = cx.tcx.interner().mk_generic_args(&args);
        Some(yelang_ty::predicate::TraitRef { def_id, args })
    } else {
        None
    }
}

fn lower_generics(tcx: &mut TyCtxt, generics: &hir::Generics) -> GenericsData {
    let params: Vec<_> = generics
        .params
        .iter()
        .map(|p| match p {
            hir::GenericParam::Type { def_id, name, .. } => GenericParamData {
                def_id: *def_id,
                ident: *name,
                kind: GenericParamKind::Type,
            },
            hir::GenericParam::Const { def_id, name, .. } => GenericParamData {
                def_id: *def_id,
                ident: *name,
                kind: GenericParamKind::Const,
            },
        })
        .collect();

    // Add predicates from inline bounds on generic parameters, e.g.
    // `fn foo<T: Clone>()`. The `Self` type of each bound is the parameter
    // itself, looked up by its DefId.
    let mut cx = CollectorCx::new(tcx, &params);

    let mut predicates: Vec<Predicate> = generics
        .where_clause
        .as_ref()
        .map(|wc| {
            wc.predicates
                .iter()
                .flat_map(|p| lower_where_predicate(&mut cx, p))
                .collect()
        })
        .unwrap_or_default();

    for param in &generics.params {
        if let hir::GenericParam::Type { def_id, bounds, .. } = param {
            let self_ty = cx
                .param_ty(*def_id)
                .unwrap_or_else(|| cx.tcx.interner().mk_ty(Ty::Error));
            for bound in bounds {
                if let Some(tr) = lower_trait_bound(&mut cx, self_ty, bound) {
                    predicates.push(Predicate::Trait(TraitPredicate {
                        trait_ref: tr,
                        polarity: yelang_ty::ty::ImplPolarity::Positive,
                    }));
                }
            }
        }
    }

    GenericsData { params, predicates }
}

fn lower_where_predicate(cx: &mut CollectorCx<'_>, pred: &hir::WherePredicate) -> Vec<Predicate> {
    match pred {
        hir::WherePredicate::TraitBound { ty, bounds } => {
            let self_ty = lower_hir_ty_id(*ty, cx);
            bounds
                .iter()
                .filter_map(|b| lower_trait_bound(cx, self_ty, b))
                .map(|trait_ref| {
                    Predicate::Trait(TraitPredicate {
                        trait_ref,
                        polarity: yelang_ty::ty::ImplPolarity::Positive,
                    })
                })
                .collect()
        }
        hir::WherePredicate::TypeEq { .. } => {
            // TODO: associated type equality constraints.
            Vec::new()
        }
    }
}

fn identity_args(
    cx: &CollectorCx<'_>,
    params: &[GenericParamData],
) -> yelang_ty::list::List<GenericArg> {
    let args: Vec<_> = params
        .iter()
        .enumerate()
        .map(|(idx, p)| match p.kind {
            GenericParamKind::Type => {
                GenericArg::Type(cx.tcx.interner().mk_ty(Ty::Param(ParamTy {
                    index: idx as u32,
                    name: p.ident.symbol,
                })))
            }
            GenericParamKind::Const => {
                // TODO: identity const args.
                GenericArg::Const(
                    cx.tcx
                        .interner()
                        .mk_const_from_parts(Const::Error, cx.tcx.interner().mk_ty(Ty::Error)),
                )
            }
        })
        .collect();
    cx.tcx.interner().mk_generic_args(&args)
}

fn lower_hir_const(tcx: &TyCtxt, c: &yelang_hir::hir::ty::Const) -> ConstId {
    use yelang_ty::ty::ConstValue;
    let ty = tcx.interner().mk_ty(Ty::Int(IntTy::I32));
    let kind = match &c.kind {
        yelang_hir::hir::ty::ConstKind::Lit { lit } => match lit {
            yelang_lexer::Literal::Int(il) => {
                let s = il.value.to_string();
                s.parse::<i128>().map(ConstValue::Int).ok()
            }
            yelang_lexer::Literal::Bool(b) => Some(ConstValue::Bool(*b)),
            _ => None,
        }
        .map_or(Const::Error, Const::Value),
        yelang_hir::hir::ty::ConstKind::Expr { .. } | yelang_hir::hir::ty::ConstKind::Err => {
            Const::Error
        }
    };
    tcx.interner().mk_const_from_parts(kind, ty)
}

// ---------------------------------------------------------------------------
// Collector lowering context
// ---------------------------------------------------------------------------

/// Lowering context used by the collector.
struct CollectorCx<'a> {
    tcx: &'a mut TyCtxt,
    param_map: FxHashMap<DefId, TyId>,
    self_ty: Option<TyId>,
}

impl<'a> CollectorCx<'a> {
    fn new(tcx: &'a mut TyCtxt, params: &[GenericParamData]) -> Self {
        let mut param_map = FxHashMap::default();
        for (idx, p) in params.iter().enumerate() {
            if let GenericParamKind::Type = p.kind {
                let ty = tcx.interner().mk_ty(Ty::Param(ParamTy {
                    index: idx as u32,
                    name: p.ident.symbol,
                }));
                param_map.insert(p.def_id, ty);
            }
        }
        Self {
            tcx,
            param_map,
            self_ty: None,
        }
    }
}

impl<'a> TyLowerCtxt for CollectorCx<'a> {
    fn interner(&self) -> &Interner {
        self.tcx.interner()
    }

    fn crate_hir(&self) -> &HirCrate {
        self.tcx.crate_hir()
    }

    fn item_ty(&self, def_id: DefId) -> Option<TyId> {
        self.tcx.item_ty(def_id)
    }

    fn param_ty(&self, def_id: DefId) -> Option<TyId> {
        self.param_map.get(&def_id).copied()
    }

    fn self_ty(&self) -> Option<TyId> {
        self.self_ty
    }

    fn lower_infer(&mut self) -> TyId {
        self.tcx.interner().mk_ty(Ty::Error)
    }

    fn lower_missing(&mut self) -> TyId {
        self.tcx.interner().mk_ty(Ty::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_hir as hir;
    use yelang_hir::hir::body::Body;
    use yelang_hir::hir::core::{FnSig, Generics, Item, ItemKind, Visibility};
    use yelang_hir::ids::BodyId;
    use yelang_hir::res::{IntTy as HirIntTy, PrimTy, Res};
    use yelang_interner::Symbol;
    use yelang_lexer::{Position, Span};
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::Ty;

    fn dummy_span() -> Span {
        Span::new(Position::default(), Position::default())
    }

    fn hir_i32(hir: &mut HirCrate) -> yelang_hir::ids::HirTyId {
        hir.alloc_ty(
            hir::hir::Ty::Path {
                res: Res::PrimTy {
                    ty: PrimTy::Int(HirIntTy::I32),
                },
                args: vec![],
            },
            dummy_span(),
        )
    }

    fn body_id(hir: &mut HirCrate) -> BodyId {
        let value = hir.alloc_expr(yelang_hir::hir::expr::Expr::Err, dummy_span());
        hir.alloc_body(
            Body {
                params: vec![],
                value,
                span: dummy_span(),
            },
            dummy_span(),
        )
    }

    #[test]
    fn collect_fn_signature() {
        let mut hir = HirCrate::new(DefId::new(1));
        let i32_ty = hir_i32(&mut hir);
        let sig = FnSig {
            inputs: vec![i32_ty],
            output: i32_ty,
            is_async: false,
            is_const: false,
            is_variadic: false,
            abi: None,
            bound_vars: vec![],
        };
        let body = body_id(&mut hir);
        let item = Item {
            def_id: DefId::new(2),
            ident: yelang_ast::Ident::new(Symbol::from(1), dummy_span()),
            kind: ItemKind::Fn {
                sig,
                body,
                generics: Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        };
        hir.items.insert(DefId::new(2), Some(item));

        let mut tcx = TyCtxt::new(hir);
        collect_crate_types(&mut tcx);

        let fn_sig = tcx
            .fn_sig(DefId::new(2))
            .expect("fn sig should be collected");
        assert_eq!(fn_sig.sig.inputs.len(), 1);
        let interner = tcx.interner();
        assert!(matches!(
            interner.ty(fn_sig.sig.output),
            Ty::Int(IntTy::I32)
        ));

        let item_ty = tcx
            .item_ty(DefId::new(2))
            .expect("item type should be collected");
        assert!(matches!(interner.ty(item_ty), Ty::FnDef(_)));
    }

    #[test]
    fn collect_struct_fields() {
        let mut hir = HirCrate::new(DefId::new(1));
        let i32_ty = hir_i32(&mut hir);
        let field = yelang_hir::hir::adt::FieldDef {
            def_id: DefId::new(10),
            ident: yelang_ast::Ident::new(Symbol::from(2), dummy_span()),
            ty: i32_ty,
            span: dummy_span(),
            vis: Visibility::Public(dummy_span()),
            attrs: vec![],
        };
        let item = Item {
            def_id: DefId::new(2),
            ident: yelang_ast::Ident::new(Symbol::from(1), dummy_span()),
            kind: ItemKind::Struct {
                data: VariantData::Struct {
                    fields: vec![field],
                },
                generics: Generics {
                    params: vec![],
                    where_clause: None,
                    span: dummy_span(),
                },
            },
            vis: Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        };
        hir.items.insert(DefId::new(2), Some(item));

        let mut tcx = TyCtxt::new(hir);
        collect_crate_types(&mut tcx);

        let adt = tcx.adt_def(DefId::new(2)).expect("adt should be collected");
        assert_eq!(adt.variants.len(), 1);
        assert_eq!(adt.variants[0].fields.len(), 1);
        let interner = tcx.interner();
        assert!(matches!(
            interner.ty(adt.variants[0].fields[0].ty),
            Ty::Int(IntTy::I32)
        ));

        let item_ty = tcx
            .item_ty(DefId::new(2))
            .expect("item type should be collected");
        assert!(matches!(interner.ty(item_ty), Ty::Adt(_, _)));
    }

    #[test]
    fn collect_where_clause_with_generic_args() {
        let mut hir = HirCrate::new(DefId::new(1));

        let t_param = DefId::new(10);
        let u_param = DefId::new(11);
        let bar_trait = DefId::new(20);

        let t_ty = hir.alloc_ty(
            hir::Ty::Path {
                res: Res::Def { def_id: t_param },
                args: vec![],
            },
            dummy_span(),
        );
        let u_ty = hir.alloc_ty(
            hir::Ty::Path {
                res: Res::Def { def_id: u_param },
                args: vec![],
            },
            dummy_span(),
        );

        let bar_bound = yelang_hir::hir::core::TraitBound {
            path: Res::Def { def_id: bar_trait },
            args: vec![yelang_hir::hir::ty::GenericArg::Type(u_ty)],
            span: dummy_span(),
        };

        let sig = FnSig {
            inputs: vec![t_ty],
            output: u_ty,
            is_async: false,
            is_const: false,
            is_variadic: false,
            abi: None,
            bound_vars: vec![],
        };
        let body = body_id(&mut hir);

        let item = Item {
            def_id: DefId::new(2),
            ident: yelang_ast::Ident::new(Symbol::from(1), dummy_span()),
            kind: ItemKind::Fn {
                sig,
                body,
                generics: Generics {
                    params: vec![
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: t_param,
                            name: yelang_ast::Ident::new(Symbol::from(10), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                        yelang_hir::hir::core::GenericParam::Type {
                            def_id: u_param,
                            name: yelang_ast::Ident::new(Symbol::from(11), dummy_span()),
                            bounds: vec![],
                            default: None,
                            span: dummy_span(),
                        },
                    ],
                    where_clause: Some(yelang_hir::hir::core::WhereClause {
                        predicates: vec![yelang_hir::hir::core::WherePredicate::TraitBound {
                            ty: t_ty,
                            bounds: vec![bar_bound],
                        }],
                        span: dummy_span(),
                    }),
                    span: dummy_span(),
                },
            },
            vis: Visibility::Public(dummy_span()),
            attrs: vec![],
            span: dummy_span(),
        };
        hir.items.insert(DefId::new(2), Some(item));

        let mut tcx = TyCtxt::new(hir);
        collect_crate_types(&mut tcx);

        let generics = tcx
            .generics_of(DefId::new(2))
            .expect("generics should exist");
        assert_eq!(generics.predicates.len(), 1);

        let interner = tcx.interner();
        let pred = &generics.predicates[0];
        let yelang_ty::predicate::Predicate::Trait(trait_pred) = pred else {
            panic!("expected trait predicate");
        };
        assert_eq!(trait_pred.trait_ref.def_id, bar_trait);
        assert_eq!(trait_pred.trait_ref.args.len(), 2);

        let self_arg = &trait_pred.trait_ref.args[0];
        let other_arg = &trait_pred.trait_ref.args[1];
        assert_eq!(
            *self_arg,
            GenericArg::Type(interner.mk_ty(Ty::Param(yelang_ty::ty::ParamTy {
                index: 0,
                name: Symbol::from(10),
            })))
        );
        assert_eq!(
            *other_arg,
            GenericArg::Type(interner.mk_ty(Ty::Param(yelang_ty::ty::ParamTy {
                index: 1,
                name: Symbol::from(11),
            })))
        );
    }
}
