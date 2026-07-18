/*! Type collection from HIR items.
 *
 * Walks HIR items, traits, and impl blocks and populates `TyCtxt` tables:
 * `item_types`, `fn_sigs`, `adt_defs`, `trait_defs`, `impl_defs`, and the
 * trait-to-impl index.
 */

use yelang_arena::{DefId, FxHashMap};
use yelang_hir::Crate as HirCrate;
use yelang_hir::hir::core as hir;
use yelang_hir::hir::item::{Item, ItemKind};
use yelang_ty::generic::GenericArg;
use yelang_hir::hir::adt::VariantData;
use yelang_ty::interner::Interner;
use yelang_ty::predicate::{Predicate, TraitPredicate};
use yelang_ty::primitive::IntTy;
use yelang_ty::ty::{AdtDef, Const, ConstKind, ParamTy, PolyFnSig, Ty, TyKind};

use crate::hir_ty_lower::lower_hir_ty_id;
use crate::lower_ctx::TyLowerCtxt;
use crate::tcx::{
    AdtDefData, AdtKind, FieldData, GenericParamData, GenericParamKind, GenericsData,
    ImplDefData, ImplItemDefData, TraitDefData, TraitItemDefData, TyCtxt,
};

/// Collect item signatures from the HIR crate into `tcx`.
pub fn collect_crate_types<'tcx>(tcx: &mut TyCtxt<'tcx>) {
    let hir = tcx.crate_hir();

    // Collect items indexed by DefId.
    for (def_id, item) in hir.items.iter_enumerated() {
        let Some(item) = item else { continue };
        collect_item(tcx, def_id, item);
    }

    // Trait definitions are stored separately.
    for (def_id, tr) in hir.traits.iter_enumerated() {
        let Some(tr) = tr else { continue };
        collect_trait(tcx, def_id, tr);
    }

    // Impl blocks are stored separately.
    let impls: Vec<_> = hir.impls.iter().cloned().collect();
    for imp in impls {
        collect_impl(tcx, &imp);
    }
}

fn collect_item<'tcx>(tcx: &mut TyCtxt<'tcx>, def_id: DefId, item: &Item) {
    match &item.kind {
        ItemKind::Fn { sig, body: _, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let poly_sig = lower_fn_sig(&mut cx, sig);
            let fn_ty = cx.tcx.interner().mk_ty(TyKind::FnDef(yelang_ty::ty::FnDef {
                def_id,
                args: identity_args(&cx, &generics_data.params),
            }));
            cx.tcx.fn_sigs.insert(def_id, poly_sig);
            cx.tcx.item_types.insert(def_id, fn_ty);
        }
        ItemKind::Struct { data, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let adt = collect_struct(&mut cx, def_id, item.ident, data, generics_data);
            let ty = cx.tcx.interner().mk_ty(TyKind::Adt(
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
            let ty = cx.tcx.interner().mk_ty(TyKind::Adt(
                AdtDef { def_id },
                identity_args(&cx, &adt.generics.params),
            ));
            cx.tcx.adt_defs.insert(def_id, adt);
            cx.tcx.item_types.insert(def_id, ty);
        }
        ItemKind::Trait { items, generics, super_traits } => {
            // Trait definitions are collected from `hir.traits`, but if an Item
            // also exists we still need an item_type placeholder.
            let _ = (items, generics, super_traits);
            tcx.item_types.insert(def_id, tcx.interner().mk_ty(TyKind::Error));
        }
        ItemKind::Impl { .. } => {
            // Impl blocks are not items with a type.
        }
        ItemKind::TyAlias { ty, generics } => {
            let generics_data = lower_generics(tcx, generics);
            let mut cx = CollectorCx::new(tcx, &generics_data.params);
            let alias_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.item_types.insert(def_id, alias_ty);
        }
        ItemKind::Const { ty, body: _ } => {
            let mut cx = CollectorCx::new(tcx, &[]);
            let const_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.item_types.insert(def_id, const_ty);
        }
        ItemKind::Static { ty, mutability: _, body: _ } => {
            let mut cx = CollectorCx::new(tcx, &[]);
            let static_ty = lower_hir_ty_id(*ty, &mut cx);
            cx.tcx.item_types.insert(def_id, static_ty);
        }
        ItemKind::Mod { .. } | ItemKind::Use { .. } => {
            // No type.
        }
    }
}

fn collect_trait<'tcx>(tcx: &mut TyCtxt<'tcx>, def_id: DefId, tr: &hir::Trait) {
    let generics_data = lower_generics(tcx, &tr.generics);
    let mut cx = CollectorCx::new(tcx, &generics_data.params);

    let supertraits: Vec<_> = tr
        .super_traits
        .iter()
        .filter_map(|t| lower_trait_ref(&mut cx, t))
        .collect();

    let items: Vec<_> = tr
        .items
        .iter()
        .map(|item| match &item.kind {
            hir::TraitItemKind::Fn { sig, default: _ } => TraitItemDefData::Fn {
                def_id, // TODO: trait items need their own DefIds
                sig: lower_fn_sig(&mut cx, sig),
            },
            hir::TraitItemKind::Const { ty, body: _ } => TraitItemDefData::Const {
                def_id, // TODO: trait items need their own DefIDs
                ty: lower_hir_ty_id(*ty, &mut cx),
            },
            hir::TraitItemKind::Type { bounds, default } => TraitItemDefData::Type {
                def_id, // TODO: trait items need their own DefIds
                bounds: bounds.iter().filter_map(|b| lower_trait_bound(&mut cx, b)).collect(),
                default: default.map(|t| lower_hir_ty_id(t, &mut cx)),
            },
        })
        .collect();

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
    cx.tcx.item_types.insert(def_id, cx.tcx.interner().mk_ty(TyKind::Error));
}

fn collect_impl<'tcx>(tcx: &mut TyCtxt<'tcx>, imp: &hir::Impl) {
    // TODO: impl blocks need their own DefIds. For now use a placeholder.
    let def_id = DefId::new(0);
    let generics_data = lower_generics(tcx, &imp.generics);
    let mut cx = CollectorCx::new(tcx, &generics_data.params);

    let self_ty = lower_hir_ty_id(imp.self_ty, &mut cx);
    cx.self_ty = Some(self_ty);

    let trait_ref = imp.of_trait.as_ref().and_then(|t| lower_trait_ref(&mut cx, t));

    let items: Vec<_> = imp
        .items
        .iter()
        .map(|item| match &item.kind {
            hir::ImplItemKind::Fn { sig, body: _ } => ImplItemDefData::Fn {
                def_id, // TODO: impl items need their own DefIds
                sig: lower_fn_sig(&mut cx, sig),
            },
            hir::ImplItemKind::Const { ty, body: _ } => ImplItemDefData::Const {
                def_id, // TODO: impl items need their own DefIds
                ty: lower_hir_ty_id(*ty, &mut cx),
            },
            hir::ImplItemKind::Type { ty } => ImplItemDefData::Type {
                def_id, // TODO: impl items need their own DefIds
                ty: lower_hir_ty_id(*ty, &mut cx),
            },
        })
        .collect();

    let impl_id = cx.tcx.impl_defs.push(ImplDefData {
        id: crate::tcx::ImplDefId::new(1), // placeholder
        def_id,
        trait_ref,
        self_ty,
        generics: generics_data,
        items,
    });

    if let Some(tr) = trait_ref {
        cx.tcx.trait_impl_index.entry(tr.def_id).or_default().push(impl_id);
    }
}

fn collect_struct<'tcx>(
    cx: &mut CollectorCx<'_, 'tcx>,
    def_id: DefId,
    ident: yelang_ast::Ident,
    data: &VariantData,
    generics: GenericsData<'tcx>,
) -> AdtDefData<'tcx> {
    let fields: Vec<_> = variant_fields(data)
        .iter()
        .enumerate()
        .map(|(idx, &(field_ident, ty_id))| FieldData {
            def_id: DefId::new((idx + 1) as u32), // TODO: field DefIds
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

fn collect_enum<'tcx>(
    cx: &mut CollectorCx<'_, 'tcx>,
    def_id: DefId,
    ident: yelang_ast::Ident,
    enum_def: &hir::EnumDef,
    generics: GenericsData<'tcx>,
) -> AdtDefData<'tcx> {
    let variants: Vec<_> = enum_def
        .variants
        .iter()
        .enumerate()
        .map(|(idx, v)| {
            let fields: Vec<_> = variant_fields(&v.data)
                .iter()
                .enumerate()
                .map(|(fidx, &(field_ident, ty_id))| FieldData {
                    def_id: DefId::new((fidx + 1) as u32), // TODO: field DefIds
                    ident: field_ident,
                    ty: lower_hir_ty_id(ty_id, cx),
                })
                .collect();
            crate::tcx::VariantData {
                def_id: DefId::new((idx + 1) as u32), // TODO: variant DefIds
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

fn variant_fields(data: &VariantData) -> Vec<(yelang_ast::Ident, yelang_hir::ids::TyId)> {
    match data {
        VariantData::Struct { fields } => fields
            .iter()
            .map(|f| (f.ident, f.ty))
            .collect(),
        VariantData::Tuple { fields } => fields
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let ident = yelang_ast::Ident::new(
                    yelang_interner::Symbol::from(idx as u32),
                    yelang_lexer::Span::default(),
                );
                (ident, f.ty)
            })
            .collect(),
        VariantData::Unit => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Lowering helpers
// ---------------------------------------------------------------------------

fn lower_fn_sig<'a, 'tcx>(cx: &mut CollectorCx<'a, 'tcx>, sig: &hir::FnSig) -> PolyFnSig<'tcx> {
    let inputs: Vec<_> = sig
        .inputs
        .iter()
        .map(|t| GenericArg::Type(lower_hir_ty_id(*t, cx)))
        .collect();
    let output = lower_hir_ty_id(sig.output, cx);
    PolyFnSig {
        sig: yelang_ty::ty::FnSig {
            inputs: cx.tcx.interner().mk_generic_args(&inputs),
            output,
        },
    }
}

fn lower_trait_ref<'a, 'tcx>(cx: &mut CollectorCx<'a, 'tcx>, tr: &hir::TraitRef) -> Option<yelang_ty::predicate::TraitRef<'tcx>> {
    lower_trait_bound(cx, &hir::TraitBound { path: tr.path.clone(), span: tr.span })
}

fn lower_trait_bound<'a, 'tcx>(cx: &mut CollectorCx<'a, 'tcx>, bound: &hir::TraitBound) -> Option<yelang_ty::predicate::TraitRef<'tcx>> {
    if let yelang_hir::res::Res::Def { def_id } = bound.path {
        // TODO: lower generic arguments of the trait bound.
        Some(yelang_ty::predicate::TraitRef {
            def_id,
            args: cx.tcx.interner().mk_generic_args(&[]),
        })
    } else {
        None
    }
}

fn lower_generics<'tcx>(tcx: &mut TyCtxt<'tcx>, generics: &hir::Generics) -> GenericsData<'tcx> {
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

    let predicates = generics
        .where_clause
        .as_ref()
        .map(|wc| wc.predicates.iter().filter_map(|p| lower_where_predicate(tcx, p)).collect())
        .unwrap_or_default();

    GenericsData { params, predicates }
}

fn lower_where_predicate<'tcx>(tcx: &mut TyCtxt<'tcx>, pred: &hir::WherePredicate) -> Option<Predicate<'tcx>> {
    match pred {
        hir::WherePredicate::TraitBound { ty, bounds } => {
            let mut cx = CollectorCx::new(tcx, &[]);
            let _self_ty = lower_hir_ty_id(*ty, &mut cx);
            // For now only support a single bound.
            bounds.first().and_then(|b| lower_trait_bound(&mut cx, b)).map(|trait_ref| {
                Predicate::Trait(TraitPredicate {
                    trait_ref,
                    polarity: yelang_ty::ty::ImplPolarity::Positive,
                })
            })
        }
        hir::WherePredicate::TypeEq { .. } => {
            // TODO: associated type equality constraints.
            None
        }
    }
}

fn identity_args<'a, 'tcx>(cx: &CollectorCx<'a, 'tcx>, params: &[GenericParamData]) -> yelang_ty::list::List<GenericArg<'tcx>> {
    let args: Vec<_> = params
        .iter()
        .map(|p| match p.kind {
            GenericParamKind::Type => {
                GenericArg::Type(cx.tcx.interner().mk_ty(TyKind::Param(ParamTy {
                    index: 0, // TODO: proper param indices
                    name: p.ident.symbol,
                })))
            }
            GenericParamKind::Const => {
                // TODO: identity const args.
                GenericArg::Const(Const {
                    kind: ConstKind::Error,
                    ty: cx.tcx.interner().mk_ty(TyKind::Error),
                })
            }
        })
        .collect();
    cx.tcx.interner().mk_generic_args(&args)
}

fn lower_hir_const<'tcx>(tcx: &TyCtxt<'tcx>, c: &yelang_hir::hir::ty::Const) -> Const<'tcx> {
    use yelang_ty::ty::ConstValue;
    let ty = tcx.interner().mk_ty(TyKind::Int(IntTy::I32));
    let kind = match &c.kind {
        yelang_hir::hir::ty::ConstKind::Lit { lit } => match lit {
            yelang_lexer::Literal::Int(il) => {
                let s = il.value.to_string();
                s.parse::<i128>().map(ConstValue::Int).ok()
            }
            yelang_lexer::Literal::Bool(b) => Some(ConstValue::Bool(*b)),
            _ => None,
        }
        .map_or(ConstKind::Error, ConstKind::Value),
        yelang_hir::hir::ty::ConstKind::Expr { .. } | yelang_hir::hir::ty::ConstKind::Err => {
            ConstKind::Error
        }
    };
    Const { kind, ty }
}

// ---------------------------------------------------------------------------
// Collector lowering context
// ---------------------------------------------------------------------------

/// Lowering context used by the collector.
struct CollectorCx<'a, 'tcx> {
    tcx: &'a mut TyCtxt<'tcx>,
    param_map: FxHashMap<DefId, Ty<'tcx>>,
    self_ty: Option<Ty<'tcx>>,
}

impl<'a, 'tcx> CollectorCx<'a, 'tcx> {
    fn new(tcx: &'a mut TyCtxt<'tcx>, params: &[GenericParamData]) -> Self {
        let mut param_map = FxHashMap::default();
        for (idx, p) in params.iter().enumerate() {
            if let GenericParamKind::Type = p.kind {
                let ty = tcx.interner().mk_ty(TyKind::Param(ParamTy {
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

impl<'a, 'tcx> TyLowerCtxt<'tcx> for CollectorCx<'a, 'tcx> {
    fn interner(&self) -> &Interner<'tcx> {
        self.tcx.interner()
    }

    fn crate_hir(&self) -> &HirCrate {
        self.tcx.crate_hir()
    }

    fn item_ty(&self, def_id: DefId) -> Option<Ty<'tcx>> {
        self.tcx.item_ty(def_id)
    }

    fn param_ty(&self, def_id: DefId) -> Option<Ty<'tcx>> {
        self.param_map.get(&def_id).copied()
    }

    fn self_ty(&self) -> Option<Ty<'tcx>> {
        self.self_ty
    }

    fn lower_infer(&mut self) -> Ty<'tcx> {
        self.tcx.interner().mk_ty(TyKind::Error)
    }

    fn lower_missing(&mut self) -> Ty<'tcx> {
        self.tcx.interner().mk_ty(TyKind::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_hir::hir::core::{FnSig, Generics, Item, ItemKind, Visibility};
    use yelang_hir::hir::body::{Body, Param};
    use yelang_hir::hir::ty::Ty as HirTy;
    use yelang_hir::ids::BodyId;
    use yelang_hir::res::{PrimTy, IntTy as HirIntTy, Res};
    use yelang_interner::Symbol;
    use yelang_lexer::{Position, Span};
    use yelang_ty::primitive::IntTy;
    use yelang_ty::ty::TyKind;

    fn dummy_span() -> Span {
        Span::new(Position::default(), Position::default())
    }

    fn hir_i32(hir: &mut HirCrate) -> yelang_hir::ids::TyId {
        hir.alloc_ty(
            HirTy::Path {
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
        let item = Item {
            def_id: DefId::new(2),
            ident: yelang_ast::Ident::new(Symbol::from(1), dummy_span()),
            kind: ItemKind::Fn {
                sig,
                body: body_id(&mut hir),
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

        let mut tcx = TyCtxt::new(&hir);
        collect_crate_types(&mut tcx);

        let fn_sig = tcx.fn_sig(DefId::new(2)).expect("fn sig should be collected");
        assert_eq!(fn_sig.sig.inputs.len(), 1);
        assert!(matches!(fn_sig.sig.output.kind(), TyKind::Int(IntTy::I32)));

        let item_ty = tcx.item_ty(DefId::new(2)).expect("item type should be collected");
        assert!(matches!(item_ty.kind(), TyKind::FnDef(_)));
    }

    #[test]
    fn collect_struct_fields() {
        let mut hir = HirCrate::new(DefId::new(1));
        let i32_ty = hir_i32(&mut hir);
        let field = yelang_hir::hir::adt::FieldDef {
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
                data: VariantData::Struct { fields: vec![field] },
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

        let mut tcx = TyCtxt::new(&hir);
        collect_crate_types(&mut tcx);

        let adt = tcx.adt_def(DefId::new(2)).expect("adt should be collected");
        assert_eq!(adt.variants.len(), 1);
        assert_eq!(adt.variants[0].fields.len(), 1);
        assert!(matches!(adt.variants[0].fields[0].ty.kind(), TyKind::Int(IntTy::I32)));

        let item_ty = tcx.item_ty(DefId::new(2)).expect("item type should be collected");
        assert!(matches!(item_ty.kind(), TyKind::Adt(_, _)));
    }
}
