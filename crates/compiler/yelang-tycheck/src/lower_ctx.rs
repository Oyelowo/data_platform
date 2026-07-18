/*! Type-lowering context trait.
 *
 * Both the signature collector and the body type checker need to lower HIR
 * types to canonical `Ty`. This trait abstracts the small amount of state
 * required for lowering: the interner, HIR crate, item-type table, self type,
 * and the handling of `_` / missing type annotations.
 */

use yelang_arena::DefId;
use yelang_hir::Crate as HirCrate;
use yelang_hir::ids::ExprId;
use yelang_ty::interner::Interner;
use yelang_ty::primitive::{FloatTy, IntTy, UintTy};
use yelang_ty::ty::{Mutability, Ty, TyKind};

/// Context used when lowering a HIR type to `Ty`.
pub trait TyLowerCtxt<'tcx> {
    /// The interner for creating canonical types.
    fn interner(&self) -> &Interner<'tcx>;

    /// The HIR crate used to look up type nodes.
    fn crate_hir(&self) -> &HirCrate;

    /// Look up the type of an item by `DefId`.
    fn item_ty(&self, def_id: DefId) -> Option<Ty<'tcx>>;

    /// Look up a type parameter by its `DefId`.
    fn param_ty(&self, def_id: DefId) -> Option<Ty<'tcx>> {
        let _ = def_id;
        None
    }

    /// The `Self` type, if inside an impl block.
    fn self_ty(&self) -> Option<Ty<'tcx>>;

    /// Lower an explicit `_` inference annotation.
    /// In bodies this becomes a fresh inference variable; in item signatures it
    /// is currently an error type (return-type inference is handled later).
    fn lower_infer(&mut self) -> Ty<'tcx>;

    /// Lower a missing type annotation.
    fn lower_missing(&mut self) -> Ty<'tcx>;

    /// Lower `typeof expr`. The default returns an error type; body checking
    /// overrides this to infer the expression's type.
    fn lower_typeof(&mut self, _expr: ExprId) -> Ty<'tcx> {
        self.mk_error()
    }

    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    fn mk_ty(&self, kind: TyKind<'tcx>) -> Ty<'tcx> {
        self.interner().mk_ty(kind)
    }

    fn mk_unit(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Tuple(yelang_ty::list::List::empty()))
    }

    fn mk_never(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Never)
    }

    fn mk_error(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Error)
    }

    fn mk_bool(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Bool)
    }

    fn mk_char(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Char)
    }

    fn mk_str(&self) -> Ty<'tcx> {
        self.mk_ty(TyKind::Str)
    }

    fn mk_int(&self, it: IntTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Int(it))
    }

    fn mk_uint(&self, ut: UintTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Uint(ut))
    }

    fn mk_float(&self, ft: FloatTy) -> Ty<'tcx> {
        self.mk_ty(TyKind::Float(ft))
    }

    fn mk_ref(&self, ty: Ty<'tcx>, mutbl: Mutability) -> Ty<'tcx> {
        self.mk_ty(TyKind::Ref(ty, mutbl))
    }
}
